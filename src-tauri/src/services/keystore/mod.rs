//! Secret storage abstraction. The production backend is the OS keychain
//! (`OsKeyStore`): macOS Keychain, Windows Credential Manager, or Linux Secret
//! Service (via keyring's per-OS backends). dev/e2e use a plaintext `FileKeyStore`
//! gated behind `debug_assertions` so a release binary can never select it.
//! Targets without a wired backend get an `Err` stub.
//!
//! Contract for every impl:
//! - `get` on a missing ref returns `Ok(None)` (not an error).
//! - `delete` on a missing ref returns `Ok(())` (idempotent).
//! - `set` overwrites an existing ref (last write wins).

/// Dataless keystore error: signals failure only, carrying nothing the command
/// layer could leak. Two variants so a "no OS keychain on this system" condition
/// (no secret) can be surfaced distinctly from a generic failure.
#[derive(Debug)]
pub enum KeyStoreError {
    /// No usable OS keychain (e.g. Linux with no Secret Service provider, a locked
    /// login keyring, or no session bus).
    NoBackend,
    /// Any other failure (the underlying error is dropped at this boundary).
    Failure,
}

pub trait KeyStore: Send + Sync {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError>;
    // Consumed by `resolve_session_env` (M10b-2a) to inject a session's account key
    // into the agent PTY env.
    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError>;
    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError>;
}

// ---- OS keychain backend (production) -------------------------------------

const SERVICE: &str = "io.n8n.uaw";

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub struct OsKeyStore;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl OsKeyStore {
    pub fn new() -> Self {
        OsKeyStore
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl Default for OsKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a keyring error to the dataless `KeyStoreError`. "No usable keychain"
/// conditions (no provider / locked / no session bus) surface as `NoStorageAccess`
/// / `PlatformFailure`; everything else is a generic `Failure`. (Both carry a boxed
/// source in keyring v3, hence the `(_)`.)
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn map_keyring_err(e: keyring::Error) -> KeyStoreError {
    match e {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            KeyStoreError::NoBackend
        }
        _ => KeyStoreError::Failure,
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl KeyStore for OsKeyStore {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref).map_err(map_keyring_err)?;
        entry.set_password(secret).map_err(map_keyring_err)
    }

    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref).map_err(map_keyring_err)?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_err(e)),
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError> {
        let entry = keyring::Entry::new(SERVICE, key_ref).map_err(map_keyring_err)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_err(e)),
        }
    }
}

// Targets with no keyring backend (not macOS/Windows/Linux): present only so the
// crate compiles there; key ops fail closed.
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub struct OsKeyStore;

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl OsKeyStore {
    pub fn new() -> Self {
        OsKeyStore
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl Default for OsKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl KeyStore for OsKeyStore {
    fn set(&self, _key_ref: &str, _secret: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::NoBackend)
    }
    fn get(&self, _key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        Err(KeyStoreError::NoBackend)
    }
    fn delete(&self, _key_ref: &str) -> Result<(), KeyStoreError> {
        Err(KeyStoreError::NoBackend)
    }
}

// ---- File backend (dev/e2e only) ------------------------------------------

#[cfg(debug_assertions)]
pub struct FileKeyStore {
    dir: std::path::PathBuf,
}

#[cfg(debug_assertions)]
impl FileKeyStore {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        let dir = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        FileKeyStore { dir }
    }

    fn path(&self, key_ref: &str) -> std::path::PathBuf {
        // key_ref is a generated UUID (safe filename); store one file per ref.
        self.dir.join(key_ref)
    }

    /// Backing directory — tests assert on the files it holds (e.g. rollback).
    #[cfg(test)]
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }
}

#[cfg(debug_assertions)]
impl KeyStore for FileKeyStore {
    fn set(&self, key_ref: &str, secret: &str) -> Result<(), KeyStoreError> {
        std::fs::write(self.path(key_ref), secret).map_err(|_| KeyStoreError::Failure)
    }

    fn get(&self, key_ref: &str) -> Result<Option<String>, KeyStoreError> {
        match std::fs::read_to_string(self.path(key_ref)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(KeyStoreError::Failure),
        }
    }

    fn delete(&self, key_ref: &str) -> Result<(), KeyStoreError> {
        match std::fs::remove_file(self.path(key_ref)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(KeyStoreError::Failure),
        }
    }
}

// ---- Resolver -------------------------------------------------------------

/// Production uses the OS keychain. In a debug build only, `UAW_KEYSTORE_DIR`
/// selects a plaintext file backend (dev + e2e). A release build has neither the
/// env branch nor the `FileKeyStore` type, so it can never downgrade to plaintext.
pub fn resolve() -> Box<dyn KeyStore> {
    #[cfg(debug_assertions)]
    {
        if let Some(dir) = std::env::var_os("UAW_KEYSTORE_DIR") {
            return Box::new(FileKeyStore::new(dir));
        }
    }
    Box::new(OsKeyStore::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("uaw-keystore-test-{}", crate::util::new_id()));
        d
    }

    #[test]
    fn file_store_set_get_delete_round_trip() {
        let store = FileKeyStore::new(temp_dir());
        store.set("ref-1", "sekret").unwrap();
        assert_eq!(store.get("ref-1").unwrap(), Some("sekret".to_string()));
        store.delete("ref-1").unwrap();
        assert_eq!(store.get("ref-1").unwrap(), None);
    }

    #[test]
    fn file_store_get_missing_returns_none() {
        let store = FileKeyStore::new(temp_dir());
        assert_eq!(store.get("nope").unwrap(), None);
    }

    #[test]
    fn file_store_delete_missing_is_ok() {
        let store = FileKeyStore::new(temp_dir());
        assert!(store.delete("nope").is_ok());
    }

    #[test]
    fn file_store_overwrite_last_wins() {
        let store = FileKeyStore::new(temp_dir());
        store.set("r", "first").unwrap();
        store.set("r", "second").unwrap();
        assert_eq!(store.get("r").unwrap(), Some("second".to_string()));
    }

    #[test]
    fn resolver_selects_file_backend_when_env_set() {
        let dir = temp_dir();
        std::env::set_var("UAW_KEYSTORE_DIR", &dir);
        let store = resolve();
        // Behavioral proof it is the file backend: a set writes into `dir`.
        store.set("probe", "value").unwrap();
        assert!(dir.join("probe").exists());
        std::env::remove_var("UAW_KEYSTORE_DIR");
    }
}
