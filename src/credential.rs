//! Secure storage for the Discord user token.
//!
//! The token is kept in the OS credential manager (macOS Keychain, Windows
//! Credential Manager, Linux Secret Service / KWallet) via the `keyring`
//! crate. On systems with no keyring backend (e.g. a headless Linux box
//! without a secret service) it falls back to a 0600-permission file so the
//! token is never world-readable.

use std::fs;
use std::path::Path;

const KEYRING_SERVICE: &str = "mewsic";
const KEYRING_USER: &str = "discord-token";

fn entry() -> Option<keyring::Entry> {
    // Overridable so tests can use a throwaway entry and never touch a real
    // mewsic credential.
    let service = std::env::var("MEWSIC_KEYRING_SERVICE")
        .unwrap_or_else(|_| KEYRING_SERVICE.to_string());
    let user = std::env::var("MEWSIC_KEYRING_USER")
        .unwrap_or_else(|_| KEYRING_USER.to_string());
    keyring::Entry::new(&service, &user).ok()
}

/// Persist the token. Prefers the OS credential manager; falls back to a
/// 0600 file in the config dir.
pub fn store_token(dir: &Path, token: &str) -> Result<(), String> {
    if let Some(entry) = entry() {
        if entry.set_password(token).is_ok() {
            let _ = fs::remove_file(dir.join("token"));
            return Ok(());
        }
    }
    fallback_store(dir, token)
}

/// Read the token back from the credential manager or the fallback file.
pub fn load_token(dir: &Path) -> Option<String> {
    if let Some(entry) = entry() {
        if let Ok(token) = entry.get_password() {
            return Some(token);
        }
    }
    fallback_load(dir)
}

/// Remove the token from both the credential manager and the fallback file.
pub fn clear_token(dir: &Path) {
    if let Some(entry) = entry() {
        let _ = entry.delete_credential();
    }
    fallback_clear(dir);
}

fn fallback_store(dir: &Path, token: &str) -> Result<(), String> {
    let _ = fs::create_dir_all(dir);
    let path = dir.join("token");
    fs::write(&path, token).map_err(|e| e.to_string())?;
    restrict_permissions(&path);
    Ok(())
}

fn fallback_load(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join("token"))
        .ok()
        .map(|raw| raw.trim().to_string())
}

fn fallback_clear(dir: &Path) {
    let _ = fs::remove_file(dir.join("token"));
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_store_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mewsic-cred-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fallback_store(&dir, "super-secret").unwrap();
        assert_eq!(fallback_load(&dir).as_deref(), Some("super-secret"));
        fallback_clear(&dir);
        assert_eq!(fallback_load(&dir), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn fallback_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("mewsic-cred-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fallback_store(&dir, "super-secret").unwrap();
        let mode = fs::metadata(dir.join("token")).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "fallback token file must not be group/world-readable");
        let _ = fs::remove_dir_all(&dir);
    }
}
