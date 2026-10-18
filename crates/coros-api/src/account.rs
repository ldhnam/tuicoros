use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::client::CorosClient;
use crate::error::Result;
use crate::request::ApiRequest;
use crate::units::date_from_int;
use crate::util::{FlexStr, Num};

/// The signed-in user's profile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Account {
    pub user_id: String,
    pub nickname: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub country_code: Option<String>,
    pub birthday: Option<NaiveDate>,
}

pub fn account_request() -> ApiRequest {
    ApiRequest::get("account/query")
}

impl CorosClient {
    /// `GET account/query`. Also the cheapest way to check a token works.
    pub async fn account(&self) -> Result<Account> {
        let raw: RawAccount = self.send(&account_request()).await?;
        Ok(raw.into())
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawAccount {
    user_id: FlexStr,
    nickname: FlexStr,
    email: FlexStr,
    head_pic: FlexStr,
    country_code: FlexStr,
    birthday: Num,
}

impl From<RawAccount> for Account {
    fn from(raw: RawAccount) -> Self {
        Account {
            user_id: raw.user_id.owned().unwrap_or_default(),
            nickname: raw.nickname.owned(),
            email: raw.email.owned(),
            avatar_url: raw.head_pic.owned(),
            country_code: raw.country_code.owned(),
            birthday: date_from_int(raw.birthday),
        }
    }
}
