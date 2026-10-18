//! The HTTP client. Every request goes through [`CorosClient::send_raw`], which
//! adds headers, limits concurrency, retries reads, and re-logs in once on `1019`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::auth::{Credentials, RawLogin, Session};
use crate::envelope;
use crate::error::{CorosError, Result};
use crate::region::Region;
use crate::request::{ApiRequest, HttpMethod};
use crate::util::body_snippet;

/// A desktop browser UA, like the Training Hub web app sends.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub region: Region,
    /// Overrides the region's base URL (tests, proxies).
    pub base_url: Option<String>,
    pub user_agent: String,
    pub timeout: Duration,
    /// Requests in flight at once. COROS rate-limits hard; keep this small.
    pub max_concurrent: usize,
    /// Extra attempts for retryable read failures (429, 5xx, timeouts).
    pub max_retries: u32,
    /// First backoff delay; doubles each retry, plus jitter.
    pub retry_base_delay: Duration,
}

impl ClientConfig {
    pub fn new(region: Region) -> Self {
        Self {
            region,
            base_url: None,
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(20),
            max_concurrent: 2,
            max_retries: 3,
            retry_base_delay: Duration::from_secs(1),
        }
    }
}

type SessionListener = Arc<dyn Fn(&Session) + Send + Sync>;

pub struct CorosClient {
    http: reqwest::Client,
    base_url: String,
    region: Region,
    session: RwLock<Option<Session>>,
    credentials: Option<Credentials>,
    listener: Option<SessionListener>,
    limiter: Semaphore,
    relogin_lock: Mutex<()>,
    max_retries: u32,
    retry_base_delay: Duration,
}

impl CorosClient {
    pub fn new(config: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(config.user_agent)
            .timeout(config.timeout)
            .build()
            .map_err(|source| CorosError::Network {
                endpoint: "client setup".to_owned(),
                source,
            })?;
        let base_url = config
            .base_url
            .unwrap_or_else(|| config.region.base_url().to_owned())
            .trim_end_matches('/')
            .to_owned();
        Ok(Self {
            http,
            base_url,
            region: config.region,
            session: RwLock::new(None),
            credentials: None,
            listener: None,
            limiter: Semaphore::new(config.max_concurrent.max(1)),
            relogin_lock: Mutex::new(()),
            max_retries: config.max_retries,
            retry_base_delay: config.retry_base_delay,
        })
    }

    /// Start with an existing session (e.g. loaded from the keychain). It must
    /// belong to this client's region.
    pub fn with_session(mut self, session: Session) -> Self {
        self.session = RwLock::new(Some(session));
        self
    }

    /// Allow transparent re-login when the token is rejected (`1019`).
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Called after every successful login, so the caller can persist the new token.
    pub fn on_session_change(
        mut self,
        listener: impl Fn(&Session) + Send + Sync + 'static,
    ) -> Self {
        self.listener = Some(Arc::new(listener));
        self
    }

    pub fn region(&self) -> Region {
        self.region
    }

    pub async fn session(&self) -> Option<Session> {
        self.session.read().await.clone()
    }

    /// `POST account/login`. Never retried automatically: COROS rate-limits logins.
    pub async fn login(&self, credentials: &Credentials) -> Result<Session> {
        let request = ApiRequest::post("account/login", credentials.login_body()).unauthenticated();
        let data = self.send_once(&request, None).await?;
        let raw: RawLogin = from_data(&request, data)?;
        let token = raw
            .access_token
            .owned()
            .ok_or_else(|| CorosError::Unexpected {
                endpoint: request.path.to_owned(),
                detail: "login succeeded but no accessToken was returned".to_owned(),
                body_snippet: String::new(),
            })?;
        let session = Session {
            token: SecretString::from(token),
            user_id: raw.user_id.owned().unwrap_or_default(),
            region: self.region,
        };
        *self.session.write().await = Some(session.clone());
        tracing::info!(region = %self.region, "signed in");
        if let Some(listener) = &self.listener {
            listener(&session);
        }
        Ok(session)
    }

    /// Send a request and return the envelope's `data` as untyped JSON.
    pub async fn send_raw(&self, request: &ApiRequest) -> Result<Option<Value>> {
        let mut attempt = 0;
        let mut relogged_in = false;
        loop {
            let token = self.current_token(request).await?;
            let result = self.send_once(request, token.as_ref()).await;
            match result {
                Err(err)
                    if err.is_token_invalid()
                        && request.idempotent
                        && !relogged_in
                        && self.credentials.is_some() =>
                {
                    relogged_in = true;
                    tracing::info!(endpoint = request.path, "token rejected, signing in again");
                    self.relogin(token.as_ref().map(|t| t.0.expose_secret()))
                        .await?;
                }
                Err(err)
                    if err.is_retryable() && request.idempotent && attempt < self.max_retries =>
                {
                    attempt += 1;
                    let delay = self.backoff(attempt);
                    tracing::warn!(endpoint = request.path, attempt, ?delay, error = %err, "retrying");
                    tokio::time::sleep(delay).await;
                }
                other => return other,
            }
        }
    }

    /// Send and decode `data` into `T`.
    pub(crate) async fn send<T: DeserializeOwned>(&self, request: &ApiRequest) -> Result<T> {
        let data = self.send_raw(request).await?;
        from_data(request, data)
    }

    /// Fetch a signed download URL (e.g. a FIT file). No token is sent, and the
    /// URL is never logged: it works as a bearer link.
    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let _permit = self.limiter.acquire().await.ok();
        let endpoint = "file download".to_owned();
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| CorosError::Network {
                endpoint: endpoint.clone(),
                source: e.without_url(),
            })?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(CorosError::Http {
                endpoint,
                status,
                body_snippet: String::new(),
            });
        }
        let bytes = response.bytes().await.map_err(|e| CorosError::Network {
            endpoint,
            source: e.without_url(),
        })?;
        Ok(bytes.to_vec())
    }

    async fn current_token(&self, request: &ApiRequest) -> Result<Option<AuthPair>> {
        if !request.authenticated {
            return Ok(None);
        }
        match &*self.session.read().await {
            Some(s) => Ok(Some(AuthPair(s.token.clone(), s.user_id.clone()))),
            None => Err(CorosError::NotAuthenticated),
        }
    }

    /// Log in again unless another task already replaced the stale token.
    async fn relogin(&self, stale_token: Option<&str>) -> Result<()> {
        let _guard = self.relogin_lock.lock().await;
        let current = self
            .session
            .read()
            .await
            .as_ref()
            .map(|s| s.token.expose_secret().to_owned());
        if current.is_some() && current.as_deref() != stale_token {
            return Ok(());
        }
        let Some(credentials) = self.credentials.clone() else {
            return Err(CorosError::TokenInvalid {
                region: self.region,
            });
        };
        self.login(&credentials).await.map(|_| ())
    }

    async fn send_once(
        &self,
        request: &ApiRequest,
        auth: Option<&AuthPair>,
    ) -> Result<Option<Value>> {
        let _permit = self.limiter.acquire().await.ok();
        let url = format!("{}/{}", self.base_url, request.path);
        let mut builder = match request.method {
            HttpMethod::Get => self.http.get(&url),
            HttpMethod::Post => self.http.post(&url),
        };
        builder = builder.headers(self.headers(auth)).query(&request.query);
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }

        let started = Instant::now();
        let network = |source| CorosError::Network {
            endpoint: request.path.to_owned(),
            source,
        };
        let response = builder.send().await.map_err(network)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(network)?;
        let result = envelope::decode(request.path, status, &body, self.region);
        tracing::debug!(
            method = ?request.method,
            endpoint = request.path,
            status,
            ms = started.elapsed().as_millis() as u64,
            result = result.as_ref().err().and_then(|e| e.result_code()).unwrap_or(envelope::SUCCESS),
            "request"
        );
        if tracing::enabled!(tracing::Level::TRACE) && request.path != "account/login" {
            tracing::trace!(endpoint = request.path, body = %body_snippet(&body, 2000), "response");
        }
        result
    }

    fn headers(&self, auth: Option<&AuthPair>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(AuthPair(token, user_id)) = auth {
            if let Ok(mut value) = HeaderValue::from_str(token.expose_secret()) {
                value.set_sensitive(true);
                headers.insert("accesstoken", value);
            }
            let yf = serde_json::json!({ "userId": user_id }).to_string();
            if let Ok(value) = HeaderValue::from_str(&yf) {
                headers.insert("yfheader", value);
            }
        }
        headers
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let exp = self
            .retry_base_delay
            .saturating_mul(1 << (attempt - 1).min(6));
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let jitter = self
            .retry_base_delay
            .mul_f64(f64::from(nanos % 500) / 1000.0);
        exp + jitter
    }
}

/// Token + userId captured for one request.
#[derive(Clone)]
struct AuthPair(SecretString, String);

fn from_data<T: DeserializeOwned>(request: &ApiRequest, data: Option<Value>) -> Result<T> {
    let data = data.ok_or_else(|| CorosError::Unexpected {
        endpoint: request.path.to_owned(),
        detail: "response has no `data`".to_owned(),
        body_snippet: String::new(),
    })?;
    // Deserialize from a reference: detail responses can be megabytes.
    T::deserialize(&data).map_err(|err| CorosError::Unexpected {
        endpoint: request.path.to_owned(),
        detail: err.to_string(),
        body_snippet: body_snippet(&data.to_string(), 300),
    })
}
