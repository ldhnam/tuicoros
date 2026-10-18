use crate::region::Region;

pub type Result<T, E = CorosError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum CorosError {
    /// The API answered with a `result` other than `0000`.
    #[error("COROS returned error {result} on {endpoint}: {message}")]
    Api {
        endpoint: String,
        result: String,
        message: String,
        tlog_id: Option<String>,
    },

    /// `1019`: the token is invalid, expired, or belongs to another region.
    #[error(
        "COROS rejected the access token on the `{region}` server (1019). Sign in again; \
         if this happens right after signing in, your account may be in another region"
    )]
    TokenInvalid { region: Region },

    /// HTTP 429. Back off and try later.
    #[error("COROS is rate limiting requests; try again in a minute")]
    RateLimited,

    /// Non-2xx status without a usable API envelope.
    #[error("HTTP {status} from {endpoint}: {body_snippet}")]
    Http {
        endpoint: String,
        status: u16,
        body_snippet: String,
    },

    #[error("network error calling {endpoint}: {source}")]
    Network {
        endpoint: String,
        #[source]
        source: reqwest::Error,
    },

    /// The response was not what we expected (not JSON, wrong shape, ...).
    #[error("unexpected response from {endpoint}: {detail} (body starts: {body_snippet})")]
    Unexpected {
        endpoint: String,
        detail: String,
        body_snippet: String,
    },

    #[error("not signed in")]
    NotAuthenticated,

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl CorosError {
    pub fn is_token_invalid(&self) -> bool {
        matches!(self, CorosError::TokenInvalid { .. })
    }

    /// Worth retrying a read after a pause.
    pub fn is_retryable(&self) -> bool {
        match self {
            CorosError::RateLimited => true,
            CorosError::Http { status, .. } => *status >= 500,
            CorosError::Network { source, .. } => source.is_timeout() || source.is_connect(),
            _ => false,
        }
    }

    /// The COROS `result` code, when the API returned one.
    pub fn result_code(&self) -> Option<&str> {
        match self {
            CorosError::Api { result, .. } => Some(result),
            CorosError::TokenInvalid { .. } => Some(crate::envelope::TOKEN_INVALID),
            _ => None,
        }
    }
}
