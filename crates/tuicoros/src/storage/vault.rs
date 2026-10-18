//! The session token and (optionally) the password hash, in a [`SecretStore`].
//!
//! Both live in **one** secret (`auth`) that is read **at most once per
//! process**: with the macOS keychain every distinct read of an item that
//! isn't "Always Allow"ed yet is a password prompt.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use color_eyre::Result;
use color_eyre::eyre::eyre;
use coros_api::{Credentials, Region, Session};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use super::secrets::SecretStore;

const AUTH_KEY: &str = "auth";
/// Before 2026-10-18 the two parts were separate secrets; read once and migrate.
const LEGACY_SESSION_KEY: &str = "session";
const LEGACY_CREDENTIALS_KEY: &str = "credentials";

/// What we persist about a signed-in session.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredSession {
    pub token: String,
    pub user_id: String,
    pub region: Region,
    pub email: Option<String>,
    pub nickname: Option<String>,
    pub saved_at: DateTime<Utc>,
}

impl std::fmt::Debug for StoredSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredSession")
            .field("token", &"[REDACTED]")
            .field("user_id", &self.user_id)
            .field("region", &self.region)
            .field("email", &self.email)
            .field("saved_at", &self.saved_at)
            .finish()
    }
}

impl StoredSession {
    pub fn from_api(session: &Session, email: Option<String>, nickname: Option<String>) -> Self {
        Self {
            token: session.token.expose_secret().to_owned(),
            user_id: session.user_id.clone(),
            region: session.region,
            email,
            nickname,
            saved_at: Utc::now(),
        }
    }

    pub fn to_api(&self) -> Session {
        Session {
            token: SecretString::from(self.token.clone()),
            user_id: self.user_id.clone(),
            region: self.region,
        }
    }

    /// Short name for the header: nickname, email, or user id.
    pub fn short_name(&self) -> String {
        self.nickname
            .clone()
            .or_else(|| self.email.clone())
            .unwrap_or_else(|| format!("user {}", self.user_id))
    }

    /// Who this is, for messages: nickname, email, or user id.
    pub fn who(&self) -> String {
        match (&self.nickname, &self.email) {
            (Some(n), Some(e)) => format!("{n} <{e}>"),
            (Some(n), None) => n.clone(),
            (None, Some(e)) => e.clone(),
            (None, None) => format!("user {}", self.user_id),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredCredentials {
    account: String,
    password_md5: String,
}

/// Everything in the one secret.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Auth {
    session: Option<StoredSession>,
    credentials: Option<StoredCredentials>,
}

#[derive(Clone)]
pub struct Vault {
    store: Arc<dyn SecretStore>,
    /// Loaded once, then kept in memory (shared by clones).
    cache: Arc<Mutex<Option<Auth>>>,
}

impl Vault {
    pub fn new(store: Box<dyn SecretStore>) -> Self {
        Self {
            store: Arc::from(store),
            cache: Arc::new(Mutex::new(None)),
        }
    }

    pub fn describe(&self) -> String {
        self.store.describe()
    }

    fn load(&self) -> Result<Auth> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| eyre!("vault lock poisoned"))?;
        if let Some(auth) = cache.as_ref() {
            return Ok(auth.clone());
        }
        let auth = match self.store.get(AUTH_KEY)? {
            Some(json) => serde_json::from_str(&json).unwrap_or_else(|err| {
                tracing::warn!(%err, "ignoring unreadable stored session");
                Auth::default()
            }),
            None => self.migrate_legacy()?,
        };
        *cache = Some(auth.clone());
        Ok(auth)
    }

    fn migrate_legacy(&self) -> Result<Auth> {
        let auth = Auth {
            session: self
                .store
                .get(LEGACY_SESSION_KEY)?
                .and_then(|j| serde_json::from_str(&j).ok()),
            credentials: self
                .store
                .get(LEGACY_CREDENTIALS_KEY)?
                .and_then(|j| serde_json::from_str(&j).ok()),
        };
        if auth.session.is_some() || auth.credentials.is_some() {
            self.store.set(AUTH_KEY, &serde_json::to_string(&auth)?)?;
            self.store.delete(LEGACY_SESSION_KEY)?;
            self.store.delete(LEGACY_CREDENTIALS_KEY)?;
            tracing::info!("moved the stored session into a single secret");
        }
        Ok(auth)
    }

    fn save(&self, update: impl FnOnce(&mut Auth)) -> Result<()> {
        let mut auth = self.load()?;
        update(&mut auth);
        if auth.session.is_none() && auth.credentials.is_none() {
            self.store.delete(AUTH_KEY)?;
        } else {
            self.store.set(AUTH_KEY, &serde_json::to_string(&auth)?)?;
        }
        *self
            .cache
            .lock()
            .map_err(|_| eyre!("vault lock poisoned"))? = Some(auth);
        Ok(())
    }

    pub fn session(&self) -> Result<Option<StoredSession>> {
        Ok(self.load()?.session)
    }

    pub fn save_session(&self, session: &StoredSession) -> Result<()> {
        self.save(|auth| auth.session = Some(session.clone()))
    }

    pub fn credentials(&self) -> Result<Option<Credentials>> {
        Ok(self
            .load()?
            .credentials
            .map(|c| Credentials::from_md5(c.account, SecretString::from(c.password_md5))))
    }

    pub fn save_credentials(&self, credentials: &Credentials) -> Result<()> {
        let stored = StoredCredentials {
            account: credentials.account.clone(),
            password_md5: credentials.password_md5().expose_secret().to_owned(),
        };
        self.save(|auth| auth.credentials = Some(stored))
    }

    pub fn forget_credentials(&self) -> Result<()> {
        if self.load()?.credentials.is_none() {
            return Ok(()); // nothing to write: avoid touching the keychain
        }
        self.save(|auth| auth.credentials = None)
    }

    /// Remove everything. Returns what was there: (session, credentials).
    pub fn clear(&self) -> Result<(bool, bool)> {
        let auth = self.load()?;
        let had = (auth.session.is_some(), auth.credentials.is_some());
        self.save(|auth| *auth = Auth::default())?;
        Ok(had)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::storage::secrets::FileStore;

    fn vault() -> (tempfile::TempDir, Vault) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = Vault::new(Box::new(FileStore::new(tmp.path().join("s.json"))));
        (tmp, vault)
    }

    fn api_session(token: &str) -> Session {
        Session {
            token: SecretString::from(token),
            user_id: "42".into(),
            region: Region::Asia,
        }
    }

    #[test]
    fn session_and_credentials_round_trip() {
        let (_tmp, vault) = vault();
        assert!(vault.session().unwrap().is_none());
        let stored = StoredSession::from_api(
            &api_session("tok"),
            Some("a@b.c".into()),
            Some("Nam".into()),
        );
        vault.save_session(&stored).unwrap();
        assert_eq!(vault.session().unwrap(), Some(stored.clone()));
        assert_eq!(stored.to_api().token.expose_secret(), "tok");
        assert_eq!(stored.who(), "Nam <a@b.c>");
        assert_eq!(stored.short_name(), "Nam");

        let creds = Credentials::from_password("a@b.c", "pw");
        vault.save_credentials(&creds).unwrap();
        let loaded = vault.credentials().unwrap().unwrap();
        assert_eq!(loaded.account, "a@b.c");
        assert_eq!(
            loaded.password_md5().expose_secret(),
            creds.password_md5().expose_secret()
        );

        vault.forget_credentials().unwrap();
        assert!(vault.credentials().unwrap().is_none());
        assert!(vault.session().unwrap().is_some(), "session kept");

        vault.save_credentials(&creds).unwrap();
        assert_eq!(vault.clear().unwrap(), (true, true));
        assert!(vault.session().unwrap().is_none() && vault.credentials().unwrap().is_none());
        assert_eq!(vault.clear().unwrap(), (false, false));
    }

    #[test]
    fn data_survives_a_new_process() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.json");
        let first = Vault::new(Box::new(FileStore::new(path.clone())));
        first
            .save_session(&StoredSession::from_api(&api_session("tok"), None, None))
            .unwrap();
        first
            .save_credentials(&Credentials::from_password("a@b.c", "pw"))
            .unwrap();
        let second = Vault::new(Box::new(FileStore::new(path)));
        assert_eq!(second.session().unwrap().unwrap().token, "tok");
        assert!(second.credentials().unwrap().is_some());
    }

    /// A store that counts reads, standing in for the keychain.
    struct Counting {
        inner: FileStore,
        reads: Arc<AtomicUsize>,
    }

    impl SecretStore for Counting {
        fn describe(&self) -> String {
            "counting".into()
        }
        fn get(&self, key: &str) -> Result<Option<String>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.get(key)
        }
        fn set(&self, key: &str, value: &str) -> Result<()> {
            self.inner.set(key, value)
        }
        fn delete(&self, key: &str) -> Result<()> {
            self.inner.delete(key)
        }
    }

    #[test]
    fn the_secret_is_read_once_per_process() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.json");
        let setup = Vault::new(Box::new(FileStore::new(path.clone())));
        setup
            .save_session(&StoredSession::from_api(&api_session("tok"), None, None))
            .unwrap();
        setup
            .save_credentials(&Credentials::from_password("a@b.c", "pw"))
            .unwrap();

        let reads = Arc::new(AtomicUsize::new(0));
        let vault = Vault::new(Box::new(Counting {
            inner: FileStore::new(path),
            reads: reads.clone(),
        }));
        let clone = vault.clone();
        vault.session().unwrap();
        vault.credentials().unwrap();
        clone.session().unwrap();
        clone
            .save_session(&StoredSession::from_api(&api_session("new"), None, None))
            .unwrap();
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "one read, however many lookups"
        );
    }

    #[test]
    fn legacy_two_secret_layout_is_migrated() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.json");
        let raw = FileStore::new(path.clone());
        let session = StoredSession::from_api(&api_session("old"), Some("a@b.c".into()), None);
        raw.set(
            LEGACY_SESSION_KEY,
            &serde_json::to_string(&session).unwrap(),
        )
        .unwrap();
        raw.set(
            LEGACY_CREDENTIALS_KEY,
            &serde_json::to_string(&StoredCredentials {
                account: "a@b.c".into(),
                password_md5: "5f4d".into(),
            })
            .unwrap(),
        )
        .unwrap();

        let vault = Vault::new(Box::new(FileStore::new(path)));
        assert_eq!(vault.session().unwrap(), Some(session));
        assert!(vault.credentials().unwrap().is_some());
        assert!(
            raw.get(LEGACY_SESSION_KEY).unwrap().is_none(),
            "old secrets removed"
        );
        assert!(
            raw.get(AUTH_KEY).unwrap().is_some(),
            "new single secret written"
        );
    }

    #[test]
    fn debug_hides_token() {
        let text = format!(
            "{:?}",
            StoredSession::from_api(&api_session("super-secret"), None, None)
        );
        assert!(!text.contains("super-secret"), "{text}");
    }
}
