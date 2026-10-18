//! Where secrets live: the OS keychain, or a `0600` file when there is none
//! (headless Linux, SSH). See docs/SECURITY_AND_CONFIG.md.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use color_eyre::Result;
use color_eyre::eyre::{WrapErr, eyre};
use serde::{Deserialize, Serialize};

use crate::paths::ensure_private_dir;

const KEYCHAIN_SERVICE: &str = "tuicoros";

/// Which secret store to use (`[secrets] backend` / `TUICOROS_SECRETS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretsBackend {
    /// Release builds: the keychain if available, otherwise the file.
    /// Development (debug) builds: the file. macOS ties a keychain "Always
    /// Allow" to the exact binary, so every rebuild would ask again.
    #[default]
    Auto,
    Keychain,
    File,
}

impl std::str::FromStr for SecretsBackend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "keychain" => Ok(Self::Keychain),
            "file" => Ok(Self::File),
            other => Err(format!(
                "unknown secrets backend `{other}` (auto, keychain, file)"
            )),
        }
    }
}

pub trait SecretStore: Send + Sync {
    /// Shown in `tuicoros status`.
    fn describe(&self) -> String;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Succeeds if the key does not exist.
    fn delete(&self, key: &str) -> Result<()>;
}

/// Pick a store. `Auto` falls back to the file store if the keychain is unusable.
pub fn open(backend: SecretsBackend, data_dir: &Path) -> Box<dyn SecretStore> {
    let file = || Box::new(FileStore::new(data_dir.join("secrets.json"))) as Box<dyn SecretStore>;
    match backend {
        SecretsBackend::File => file(),
        SecretsBackend::Keychain => Box::new(KeychainStore),
        SecretsBackend::Auto if cfg!(debug_assertions) => file(),
        SecretsBackend::Auto => match keyring::Entry::store_status() {
            Ok(()) => Box::new(KeychainStore),
            Err(err) => {
                tracing::warn!(%err, "keychain unavailable, storing secrets in a private file");
                file()
            }
        },
    }
}

pub struct KeychainStore;

impl KeychainStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN_SERVICE, key).wrap_err("cannot open the system keychain")
    }
}

impl SecretStore for KeychainStore {
    fn describe(&self) -> String {
        "system keychain".to_owned()
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(eyre!(err)).wrap_err("cannot read from the system keychain"),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?
            .set_password(value)
            .wrap_err("cannot write to the system keychain")
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(eyre!(err)).wrap_err("cannot delete from the system keychain"),
        }
    }
}

/// A JSON object of key → value in a file only the owner can read.
pub struct FileStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) if text.trim().is_empty() => Ok(BTreeMap::new()),
            Ok(text) => serde_json::from_str(&text).wrap_err_with(|| {
                format!(
                    "{} is corrupted; delete it and sign in again",
                    self.path.display()
                )
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(err) => Err(err).wrap_err_with(|| format!("cannot read {}", self.path.display())),
        }
    }

    /// Write via a temp file + rename, created `0600` from the start.
    fn write(&self, map: &BTreeMap<String, String>) -> Result<()> {
        let dir = self
            .path
            .parent()
            .ok_or_else(|| eyre!("secrets file has no parent directory"))?;
        ensure_private_dir(dir)?;
        if map.is_empty() {
            return match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(err).wrap_err("cannot remove secrets file"),
            };
        }
        let tmp = self.path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .wrap_err_with(|| format!("cannot write {}", tmp.display()))?;
        file.write_all(serde_json::to_string_pretty(map)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &self.path).wrap_err("cannot replace secrets file")?;
        Ok(())
    }
}

impl SecretStore for FileStore {
    fn describe(&self) -> String {
        format!("private file {}", self.path.display())
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| eyre!("secrets lock poisoned"))?;
        Ok(self.read()?.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| eyre!("secrets lock poisoned"))?;
        let mut map = self.read()?;
        map.insert(key.to_owned(), value.to_owned());
        self.write(&map)
    }

    fn delete(&self, key: &str) -> Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| eyre!("secrets lock poisoned"))?;
        let mut map = self.read()?;
        if map.remove(key).is_some() {
            self.write(&map)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trip_and_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FileStore::new(tmp.path().join("sub/secrets.json"));
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "1").unwrap();
        store.set("b", "2").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("1"));
        store.delete("a").unwrap();
        store.delete("missing").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.get("b").unwrap().as_deref(), Some("2"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path().join("sub/secrets.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        store.delete("b").unwrap();
        assert!(
            !tmp.path().join("sub/secrets.json").exists(),
            "empty store removes the file"
        );
    }

    #[test]
    fn corrupted_file_is_a_clear_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json");
        std::fs::write(&path, "{not json").unwrap();
        let err = FileStore::new(path).get("a").unwrap_err();
        assert!(format!("{err:#}").contains("corrupted"));
    }

    #[test]
    fn backend_parsing() {
        assert_eq!("FILE".parse::<SecretsBackend>(), Ok(SecretsBackend::File));
        assert!("vault".parse::<SecretsBackend>().is_err());
    }
}
