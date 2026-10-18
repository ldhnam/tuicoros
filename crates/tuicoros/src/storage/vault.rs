//! The session token and (optionally) the password hash, in a [`SecretStore`].

use std::sync::Arc;

use chrono::{DateTime, Utc};
use color_eyre::Result;
use coros_api::{Credentials, Region, Session};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

use super::secrets::SecretStore;

const SESSION_KEY: &str = "session";
const CREDENTIALS_KEY: &str = "credentials";

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

#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    account: String,
    password_md5: String,
}

#[derive(Clone)]
pub struct Vault {
    store: Arc<dyn SecretStore>,
}

impl Vault {
    pub fn new(store: Box<dyn SecretStore>) -> Self {
        Self {
            store: Arc::from(store),
        }
    }

    pub fn describe(&self) -> String {
        self.store.describe()
    }

    /// A stored session that can't be parsed (old format) counts as none.
    pub fn session(&self) -> Result<Option<StoredSession>> {
        Ok(self
            .store
            .get(SESSION_KEY)?
            .and_then(|json| match serde_json::from_str(&json) {
                Ok(session) => Some(session),
                Err(err) => {
                    tracing::warn!(%err, "ignoring unreadable stored session");
                    None
                }
            }))
    }

    pub fn save_session(&self, session: &StoredSession) -> Result<()> {
        self.store
            .set(SESSION_KEY, &serde_json::to_string(session)?)
    }

    pub fn credentials(&self) -> Result<Option<Credentials>> {
        Ok(self.store.get(CREDENTIALS_KEY)?.and_then(|json| {
            serde_json::from_str::<StoredCredentials>(&json)
                .ok()
                .map(|c| Credentials::from_md5(c.account, SecretString::from(c.password_md5)))
        }))
    }

    pub fn save_credentials(&self, credentials: &Credentials) -> Result<()> {
        let stored = StoredCredentials {
            account: credentials.account.clone(),
            password_md5: credentials.password_md5().expose_secret().to_owned(),
        };
        self.store
            .set(CREDENTIALS_KEY, &serde_json::to_string(&stored)?)
    }

    pub fn forget_credentials(&self) -> Result<()> {
        self.store.delete(CREDENTIALS_KEY)
    }

    /// Remove everything. Returns what was there: (session, credentials).
    pub fn clear(&self) -> Result<(bool, bool)> {
        let had_session = self.store.get(SESSION_KEY)?.is_some();
        let had_credentials = self.store.get(CREDENTIALS_KEY)?.is_some();
        self.store.delete(SESSION_KEY)?;
        self.store.delete(CREDENTIALS_KEY)?;
        Ok((had_session, had_credentials))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::secrets::FileStore;

    fn vault() -> (tempfile::TempDir, Vault) {
        let tmp = tempfile::tempdir().unwrap();
        let vault = Vault::new(Box::new(FileStore::new(tmp.path().join("s.json"))));
        (tmp, vault)
    }

    #[test]
    fn session_and_credentials_round_trip() {
        let (_tmp, vault) = vault();
        assert!(vault.session().unwrap().is_none());
        let api = Session {
            token: SecretString::from("tok"),
            user_id: "42".into(),
            region: Region::Asia,
        };
        let stored = StoredSession::from_api(&api, Some("a@b.c".into()), Some("Nam".into()));
        vault.save_session(&stored).unwrap();
        assert_eq!(vault.session().unwrap(), Some(stored.clone()));
        assert_eq!(stored.to_api().token.expose_secret(), "tok");
        assert_eq!(stored.who(), "Nam <a@b.c>");

        let creds = Credentials::from_password("a@b.c", "pw");
        vault.save_credentials(&creds).unwrap();
        let loaded = vault.credentials().unwrap().unwrap();
        assert_eq!(loaded.account, "a@b.c");
        assert_eq!(
            loaded.password_md5().expose_secret(),
            creds.password_md5().expose_secret()
        );

        assert_eq!(vault.clear().unwrap(), (true, true));
        assert!(vault.session().unwrap().is_none() && vault.credentials().unwrap().is_none());
        assert_eq!(vault.clear().unwrap(), (false, false));
    }

    #[test]
    fn debug_hides_token() {
        let api = Session {
            token: SecretString::from("super-secret"),
            user_id: "1".into(),
            region: Region::Us,
        };
        let text = format!("{:?}", StoredSession::from_api(&api, None, None));
        assert!(!text.contains("super-secret"), "{text}");
    }
}
