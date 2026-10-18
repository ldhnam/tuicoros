//! Shared helpers for the wiremock-based tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Duration;

use coros_api::{ClientConfig, CorosClient, Credentials, Region, Session};
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const TOKEN: &str = "test-token-old";
pub const USER_ID: &str = "123456789012";

pub fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()))
}

pub fn json_response(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(fixture(name), "application/json")
}

pub fn config(server: &MockServer) -> ClientConfig {
    ClientConfig {
        base_url: Some(server.uri()),
        retry_base_delay: Duration::ZERO,
        ..ClientConfig::new(Region::Asia)
    }
}

pub fn session() -> Session {
    Session {
        token: SecretString::from(TOKEN),
        user_id: USER_ID.to_owned(),
        region: Region::Asia,
    }
}

/// A client that is already signed in.
pub fn signed_in(server: &MockServer) -> CorosClient {
    CorosClient::new(config(server))
        .unwrap()
        .with_session(session())
}

pub fn credentials() -> Credentials {
    Credentials::from_password("runner@example.invalid", "password")
}

/// Serve `fixture` for `METHOD /path`.
pub async fn mount(server: &MockServer, http_method: &str, url_path: &str, fixture_name: &str) {
    Mock::given(method(http_method))
        .and(path(url_path))
        .respond_with(json_response(fixture_name))
        .mount(server)
        .await;
}

pub async fn requests_to(server: &MockServer, url_path: &str) -> Vec<wiremock::Request> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.url.path() == url_path)
        .collect()
}

pub fn header(request: &wiremock::Request, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}
