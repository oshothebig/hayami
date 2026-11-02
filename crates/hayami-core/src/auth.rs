//! Authentication: obtaining a GitHub token from `gh` or from a manually
//! entered value stored in the keychain.
//!
//! The token only lives in memory wrapped in [`Token`], which never prints its
//! value through `Debug` or `Display`.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use secrecy::{ExposeSecret, SecretString};

/// A GitHub access token. The value is never printed.
#[derive(Clone)]
pub struct Token(SecretString);

impl Token {
    /// Wraps a token value. Surrounding whitespace is trimmed.
    pub fn new(value: impl AsRef<str>) -> Option<Self> {
        let trimmed = value.as_ref().trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(Self(SecretString::from(trimmed.to_owned())))
        }
    }

    /// Returns the raw token. Only call this at the point the token is handed
    /// to a transport (HTTP header or git credential callback).
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([REDACTED])")
    }
}

/// Errors from obtaining a token. None of the variants contain the token.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// `gh` was not found on the login shell `PATH`.
    #[error("`gh` was not found on PATH")]
    GhNotFound,
    /// `gh auth token` failed, typically because the user is not logged in.
    #[error("`gh auth token` failed. Run `gh auth login` and reload")]
    GhFailed,
    /// No token is stored for manual authentication.
    #[error("no token is stored; enter a personal access token")]
    NoStoredToken,
    /// The API rejected the token even after re-obtaining it.
    #[error("GitHub rejected the token. Run `gh auth login` and reload")]
    LoginRequired,
    /// The API rejected a manually entered token.
    #[error("GitHub rejected the stored token; enter a new personal access token")]
    ManualTokenRejected,
    /// The keychain could not be accessed.
    #[error("keychain error: {0}")]
    Keychain(String),
}

/// Something that can produce a token on demand.
pub trait TokenSource: Send + Sync {
    /// Obtains a fresh token.
    fn fetch(&self) -> Result<Token, AuthError>;
    /// Error to report when the API keeps rejecting tokens from this source.
    fn rejected_error(&self) -> AuthError;
}

/// Storage for a manually entered token (the macOS keychain in the app).
pub trait CredentialStore: Send + Sync {
    /// Loads the stored token, if any.
    fn load(&self) -> Result<Option<Token>, AuthError>;
    /// Stores a token, replacing any previous one.
    fn save(&self, token: &Token) -> Result<(), AuthError>;
}

/// Obtains tokens by running `gh auth token`.
#[derive(Debug, Clone)]
pub struct GhCli {
    program: PathBuf,
    path_env: Option<OsString>,
}

impl GhCli {
    /// Uses `program` as the `gh` executable. `path_env` is passed as `PATH`
    /// so that `gh` can find its own helpers when launched from Finder.
    pub fn new(program: PathBuf, path_env: Option<OsString>) -> Self {
        Self { program, path_env }
    }

    /// Path of the `gh` executable.
    pub fn program(&self) -> &Path {
        &self.program
    }
}

impl TokenSource for GhCli {
    fn fetch(&self) -> Result<Token, AuthError> {
        let mut cmd = Command::new(&self.program);
        cmd.args(["auth", "token", "--hostname", "github.com"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(path) = &self.path_env {
            cmd.env("PATH", path);
        }
        let output = cmd.output().map_err(|_| AuthError::GhFailed)?;
        if !output.status.success() {
            return Err(AuthError::GhFailed);
        }
        let stdout = String::from_utf8(output.stdout).map_err(|_| AuthError::GhFailed)?;
        Token::new(stdout).ok_or(AuthError::GhFailed)
    }

    fn rejected_error(&self) -> AuthError {
        AuthError::LoginRequired
    }
}

/// Token source backed by a [`CredentialStore`].
pub struct StoredToken<S: CredentialStore> {
    store: S,
}

impl<S: CredentialStore> StoredToken<S> {
    /// Creates a source reading from `store`.
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S: CredentialStore> TokenSource for StoredToken<S> {
    fn fetch(&self) -> Result<Token, AuthError> {
        self.store.load()?.ok_or(AuthError::NoStoredToken)
    }

    fn rejected_error(&self) -> AuthError {
        AuthError::ManualTokenRejected
    }
}

/// Holds the current token in memory and re-obtains it on demand.
pub struct TokenProvider {
    source: Box<dyn TokenSource>,
    current: Mutex<Option<Token>>,
}

impl fmt::Debug for TokenProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenProvider").finish_non_exhaustive()
    }
}

impl TokenProvider {
    /// Creates a provider. No token is fetched until [`Self::token`] is called.
    pub fn new(source: Box<dyn TokenSource>) -> Self {
        Self {
            source,
            current: Mutex::new(None),
        }
    }

    /// Returns the cached token, fetching one if none is cached.
    pub fn token(&self) -> Result<Token, AuthError> {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(token) = current.as_ref() {
            return Ok(token.clone());
        }
        let token = self.source.fetch()?;
        *current = Some(token.clone());
        Ok(token)
    }

    /// Drops the cached token and fetches a new one.
    pub fn refresh(&self) -> Result<Token, AuthError> {
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        *current = None;
        let token = self.source.fetch()?;
        *current = Some(token.clone());
        Ok(token)
    }

    /// Error to show when the API rejects even a refreshed token.
    pub fn rejected_error(&self) -> AuthError {
        self.source.rejected_error()
    }
}

/// How hayami authenticates, decided at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthMethod {
    /// `gh` is available; tokens come from `gh auth token`.
    Gh(PathBuf),
    /// `gh` is not available; the user enters a token stored in the keychain.
    Manual,
}

/// Decides the authentication method by searching `gh` on `path_env`.
pub fn choose_auth_method(path_env: Option<&OsStr>) -> AuthMethod {
    match path_env.and_then(|p| find_executable("gh", p)) {
        Some(gh) => AuthMethod::Gh(gh),
        None => AuthMethod::Manual,
    }
}

/// Searches `name` in the directories listed in `path_env`.
pub fn find_executable(name: &str, path_env: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_env)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

const PATH_MARKER: &str = "__HAYAMI_PATH__=";

/// Returns the `PATH` of the user's login shell.
///
/// Apps launched from Finder inherit a minimal `PATH` without Homebrew, so
/// the login shell is asked for its `PATH`. Falls back to the process `PATH`
/// (with common Homebrew locations appended) when that fails.
pub fn login_shell_path() -> Option<OsString> {
    let shell = std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| OsString::from("/bin/zsh"));
    let from_shell = Command::new(&shell)
        .args(["-l", "-c", &format!("printf '%s' \"{PATH_MARKER}$PATH\"")])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| parse_marked_path(&String::from_utf8_lossy(&o.stdout)));
    from_shell.or_else(|| {
        let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let extra = PathBuf::from(extra);
            if !paths.contains(&extra) {
                paths.push(extra);
            }
        }
        std::env::join_paths(paths).ok()
    })
}

/// Extracts the `PATH` printed after the marker, ignoring anything that shell
/// startup files printed before it.
fn parse_marked_path(stdout: &str) -> Option<OsString> {
    let start = stdout.rfind(PATH_MARKER)? + PATH_MARKER.len();
    let path = stdout[start..].trim();
    (!path.is_empty()).then(|| OsString::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn token_debug_does_not_leak() {
        let token = Token::new("ghp_supersecretvalue").unwrap();
        let debug = format!("{token:?}");
        assert!(!debug.contains("supersecret"), "{debug}");
        assert_eq!(debug, "Token([REDACTED])");
        let wrapped = format!("{:?}", Some(token.clone()));
        assert!(!wrapped.contains("supersecret"));
        assert_eq!(token.expose(), "ghp_supersecretvalue");
    }

    #[test]
    fn token_rejects_blank_and_trims() {
        assert!(Token::new("  \n").is_none());
        assert_eq!(Token::new(" abc\n").unwrap().expose(), "abc");
    }

    #[test]
    fn auth_error_messages_do_not_contain_token() {
        for err in [
            AuthError::GhNotFound,
            AuthError::GhFailed,
            AuthError::NoStoredToken,
            AuthError::LoginRequired,
            AuthError::ManualTokenRejected,
        ] {
            assert!(!err.to_string().contains("ghp_"));
        }
        assert!(
            AuthError::LoginRequired
                .to_string()
                .contains("gh auth login")
        );
    }

    #[cfg(unix)]
    fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn chooses_manual_when_gh_is_missing() {
        let empty = tempfile::tempdir().unwrap();
        let path = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(choose_auth_method(Some(&path)), AuthMethod::Manual);
        assert_eq!(choose_auth_method(None), AuthMethod::Manual);
    }

    #[cfg(unix)]
    #[test]
    fn chooses_gh_when_found_on_path() {
        let empty = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let gh = write_script(bin.path(), "gh", "echo token");
        // A non-executable file named gh must be skipped.
        std::fs::write(empty.path().join("gh"), "not executable").unwrap();
        let path = std::env::join_paths([empty.path(), bin.path()]).unwrap();
        assert_eq!(choose_auth_method(Some(&path)), AuthMethod::Gh(gh));
    }

    #[cfg(unix)]
    #[test]
    fn gh_cli_reads_token_from_stdout() {
        let bin = tempfile::tempdir().unwrap();
        let gh = write_script(
            bin.path(),
            "gh",
            r#"[ "$1 $2" = "auth token" ] || exit 2; echo "gho_fromgh""#,
        );
        let source = GhCli::new(gh, None);
        assert_eq!(source.fetch().unwrap().expose(), "gho_fromgh");
    }

    #[cfg(unix)]
    #[test]
    fn gh_cli_failure_maps_to_gh_failed() {
        let bin = tempfile::tempdir().unwrap();
        let gh = write_script(bin.path(), "gh", "echo 'not logged in' >&2; exit 1");
        let source = GhCli::new(gh, None);
        assert_eq!(source.fetch().unwrap_err(), AuthError::GhFailed);
    }

    #[test]
    fn parses_marked_path_after_shell_noise() {
        assert_eq!(
            parse_marked_path("welcome!\n__HAYAMI_PATH__=/opt/homebrew/bin:/usr/bin"),
            Some(OsString::from("/opt/homebrew/bin:/usr/bin"))
        );
        assert_eq!(parse_marked_path("no marker"), None);
        assert_eq!(parse_marked_path("__HAYAMI_PATH__="), None);
    }

    struct Counting {
        calls: Arc<AtomicUsize>,
    }

    impl TokenSource for Counting {
        fn fetch(&self) -> Result<Token, AuthError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Token::new(format!("t{n}")).unwrap())
        }
        fn rejected_error(&self) -> AuthError {
            AuthError::LoginRequired
        }
    }

    #[test]
    fn provider_caches_until_refresh() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = TokenProvider::new(Box::new(Counting {
            calls: calls.clone(),
        }));
        assert_eq!(provider.token().unwrap().expose(), "t0");
        assert_eq!(provider.token().unwrap().expose(), "t0");
        assert_eq!(provider.refresh().unwrap().expose(), "t1");
        assert_eq!(provider.token().unwrap().expose(), "t1");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    struct MemoryStore(Mutex<Option<Token>>);

    impl CredentialStore for MemoryStore {
        fn load(&self) -> Result<Option<Token>, AuthError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save(&self, token: &Token) -> Result<(), AuthError> {
            *self.0.lock().unwrap() = Some(token.clone());
            Ok(())
        }
    }

    #[test]
    fn stored_token_source() {
        let store = MemoryStore(Mutex::new(None));
        let source = StoredToken::new(store);
        assert_eq!(source.fetch().unwrap_err(), AuthError::NoStoredToken);
        source.store.save(&Token::new("manual").unwrap()).unwrap();
        assert_eq!(source.fetch().unwrap().expose(), "manual");
        assert_eq!(source.rejected_error(), AuthError::ManualTokenRejected);
    }
}
