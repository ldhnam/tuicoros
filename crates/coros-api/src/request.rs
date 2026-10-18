use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

/// A description of one API call. Built by the functions in [`crate::requests`],
/// sent by [`crate::CorosClient`].
#[derive(Debug, Clone, PartialEq)]
pub struct ApiRequest {
    pub method: HttpMethod,
    /// Path relative to the regional base URL, without a leading slash.
    pub path: &'static str,
    pub query: Vec<(&'static str, String)>,
    pub body: Option<Value>,
    /// Safe to send again automatically (reads). Writes must never be retried.
    pub idempotent: bool,
    /// Needs the access token.
    pub authenticated: bool,
}

impl ApiRequest {
    /// An authenticated, idempotent GET.
    pub fn get(path: &'static str) -> Self {
        Self {
            method: HttpMethod::Get,
            path,
            query: Vec::new(),
            body: None,
            idempotent: true,
            authenticated: true,
        }
    }

    /// An authenticated POST. **Not** idempotent unless marked with [`Self::read_only`].
    pub fn post(path: &'static str, body: Value) -> Self {
        Self {
            method: HttpMethod::Post,
            path,
            query: Vec::new(),
            body: Some(body),
            idempotent: false,
            authenticated: true,
        }
    }

    /// Some reads are POSTs (e.g. `activity/detail/query`); mark them retryable.
    pub fn read_only(mut self) -> Self {
        self.idempotent = true;
        self
    }

    pub fn query(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.query.push((key, value.into()));
        self
    }

    pub(crate) fn unauthenticated(mut self) -> Self {
        self.authenticated = false;
        self
    }
}
