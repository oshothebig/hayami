//! Location of hayami's local bare repositories.
//!
//! Repositories live in `$XDG_CACHE_HOME/hayami/repos/<host>/<owner>/<repo>.git`
//! (falling back to `~/.cache` when `XDG_CACHE_HOME` is unset, empty or not
//! absolute). This intentionally does not use `~/Library` on macOS.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Resolves the cache base directory from the given environment values.
///
/// `xdg_cache_home` is the value of `XDG_CACHE_HOME`; `home` is the user's
/// home directory. Following the XDG spec, relative values are ignored.
pub fn cache_home(xdg_cache_home: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg_cache_home
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty() && p.is_absolute())
        .or_else(|| {
            home.filter(|h| !h.as_os_str().is_empty())
                .map(|h| h.join(".cache"))
        })
}

/// Directory holding all bare repositories, from the process environment.
pub fn repos_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    cache_home(std::env::var_os("XDG_CACHE_HOME"), home).map(|c| repos_root_in(&c))
}

/// `<cache>/hayami/repos`.
pub fn repos_root_in(cache_home: &Path) -> PathBuf {
    cache_home.join(crate::APP_NAME).join("repos")
}

/// Path of the bare repository of `owner/repo` on `host` under `root`.
///
/// Returns `None` if a component is not a plain path segment (for example
/// `..` or contains a slash), so a hostile name cannot escape `root`.
pub fn repo_dir(root: &Path, host: &str, owner: &str, repo: &str) -> Option<PathBuf> {
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && !s.contains(['/', '\\', '\0'])
            && !s.starts_with('-')
    };
    (valid(host) && valid(owner) && valid(repo))
        .then(|| root.join(host).join(owner).join(format!("{repo}.git")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_xdg_cache_home_when_set() {
        assert_eq!(
            cache_home(Some("/xdg".into()), Some("/home/u".into())),
            Some(PathBuf::from("/xdg"))
        );
    }

    #[test]
    fn falls_back_to_home_cache_when_unset() {
        assert_eq!(
            cache_home(None, Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.cache"))
        );
    }

    #[test]
    fn falls_back_when_empty() {
        assert_eq!(
            cache_home(Some("".into()), Some("/Users/u".into())),
            Some(PathBuf::from("/Users/u/.cache"))
        );
    }

    #[test]
    fn ignores_relative_xdg_value() {
        assert_eq!(
            cache_home(Some("relative/cache".into()), Some("/home/u".into())),
            Some(PathBuf::from("/home/u/.cache"))
        );
    }

    #[test]
    fn none_without_any_home() {
        assert_eq!(cache_home(None, None), None);
        assert_eq!(cache_home(Some("".into()), Some("".into())), None);
    }

    #[test]
    fn repo_dir_layout() {
        let root = repos_root_in(Path::new("/c"));
        assert_eq!(root, PathBuf::from("/c/hayami/repos"));
        assert_eq!(
            repo_dir(&root, "github.com", "octo", "hello"),
            Some(PathBuf::from("/c/hayami/repos/github.com/octo/hello.git"))
        );
    }

    #[test]
    fn repo_dir_rejects_traversal() {
        let root = Path::new("/r");
        assert_eq!(repo_dir(root, "github.com", "..", "x"), None);
        assert_eq!(repo_dir(root, "github.com", "a/b", "x"), None);
        assert_eq!(repo_dir(root, "github.com", "a", ""), None);
    }
}
