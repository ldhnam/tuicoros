use md5::{Digest, Md5};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::region::Region;
use crate::util::FlexStr;

/// What `account/login` needs. The password is only ever held as its MD5,
/// which is what COROS receives (docs/SECURITY_AND_CONFIG.md §1).
#[derive(Clone)]
pub struct Credentials {
    pub account: String,
    password_md5: SecretString,
}

impl Credentials {
    /// Hash `password` immediately; the plaintext is not kept.
    pub fn from_password(account: impl Into<String>, password: &str) -> Self {
        Self {
            account: account.into(),
            password_md5: SecretString::from(md5_hex(password)),
        }
    }

    /// From a previously stored hash.
    pub fn from_md5(account: impl Into<String>, password_md5: SecretString) -> Self {
        Self {
            account: account.into(),
            password_md5,
        }
    }

    pub fn password_md5(&self) -> &SecretString {
        &self.password_md5
    }

    pub(crate) fn login_body(&self) -> serde_json::Value {
        serde_json::json!({
            "account": self.account,
            "accountType": 2,
            "pwd": self.password_md5.expose_secret(),
        })
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("account", &self.account)
            .field("password_md5", &"[REDACTED]")
            .finish()
    }
}

/// A signed-in session. The token is only valid on `region`'s server.
#[derive(Debug, Clone)]
pub struct Session {
    pub token: SecretString,
    pub user_id: String,
    pub region: Region,
}

/// Lowercase hex MD5, as COROS expects for `pwd`.
pub fn md5_hex(input: &str) -> String {
    let digest = Md5::digest(input.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawLogin {
    #[serde(default)]
    pub(crate) access_token: FlexStr,
    #[serde(default)]
    pub(crate) user_id: FlexStr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_known_vectors() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("password"), "5f4dcc3b5aa765d61d8327deb882cf99");
        assert_eq!(md5_hex("päss"), md5_hex("päss"));
    }

    #[test]
    fn login_body_shape() {
        let c = Credentials::from_password("me@example.com", "password");
        assert_eq!(
            c.login_body(),
            serde_json::json!({"account": "me@example.com", "accountType": 2, "pwd": "5f4dcc3b5aa765d61d8327deb882cf99"})
        );
    }

    #[test]
    fn debug_never_shows_secrets() {
        let c = Credentials::from_password("me@example.com", "password");
        let s = Session {
            token: SecretString::from("tok-123"),
            user_id: "1".into(),
            region: Region::Asia,
        };
        let text = format!("{c:?} {s:?}");
        assert!(
            !text.contains("5f4dcc") && !text.contains("tok-123"),
            "{text}"
        );
    }
}
