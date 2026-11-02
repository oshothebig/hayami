//! Storage of a manually entered token in the macOS keychain.

use hayami_core::auth::{AuthError, CredentialStore, Token};

const SERVICE: &str = "dev.hayami.github-token";
const ACCOUNT: &str = "github.com";

/// The login keychain of the current user.
#[derive(Debug, Clone, Copy, Default)]
pub struct Keychain;

#[cfg(target_os = "macos")]
impl CredentialStore for Keychain {
    fn load(&self) -> Result<Option<Token>, AuthError> {
        use security_framework::passwords::get_generic_password;
        // errSecItemNotFound
        const NOT_FOUND: i32 = -25300;
        match get_generic_password(SERVICE, ACCOUNT) {
            Ok(bytes) => Ok(String::from_utf8(bytes).ok().and_then(Token::new)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(AuthError::Keychain(format!("OSStatus {}", e.code()))),
        }
    }

    fn save(&self, token: &Token) -> Result<(), AuthError> {
        security_framework::passwords::set_generic_password(
            SERVICE,
            ACCOUNT,
            token.expose().as_bytes(),
        )
        .map_err(|e| AuthError::Keychain(format!("OSStatus {}", e.code())))
    }
}

// Lets the app type-check on other platforms; hayami only runs on macOS.
#[cfg(not(target_os = "macos"))]
impl CredentialStore for Keychain {
    fn load(&self) -> Result<Option<Token>, AuthError> {
        let _ = (SERVICE, ACCOUNT);
        Ok(None)
    }

    fn save(&self, _token: &Token) -> Result<(), AuthError> {
        Err(AuthError::Keychain(
            "keychain is only available on macOS".into(),
        ))
    }
}
