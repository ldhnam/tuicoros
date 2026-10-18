//! HTTP behaviour of `CorosClient`: headers, login, re-login, retries, limits.
#![allow(clippy::unwrap_used, clippy::expect_used)] // panicking is how tests fail

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use common::*;
use coros_api::{
    ActivityQuery, ApiRequest, ClientConfig, CorosClient, CorosError, DateRange, LabelId,
    SportType, md5_hex,
};
use secrecy::ExposeSecret;
use wiremock::matchers::{body_json, header as header_is, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn login_sends_md5_and_stores_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/account/login"))
        .and(body_json(serde_json::json!({
            "account": "runner@example.invalid",
            "accountType": 2,
            "pwd": md5_hex("password"),
        })))
        .respond_with(json_response("login/synthetic.json"))
        .expect(1)
        .mount(&server)
        .await;

    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in_listener = seen.clone();
    let client = CorosClient::new(config(&server))
        .unwrap()
        .on_session_change(move |s| {
            seen_in_listener
                .lock()
                .unwrap()
                .push(s.token.expose_secret().to_owned())
        });

    let session = client.login(&credentials()).await.unwrap();
    assert_eq!(session.token.expose_secret(), "SYNTHETIC-TOKEN-NEW");
    assert_eq!(session.user_id, "123456789012");
    assert_eq!(
        *seen.lock().unwrap(),
        vec!["SYNTHETIC-TOKEN-NEW".to_owned()]
    );
    assert!(client.session().await.is_some());

    let login = &requests_to(&server, "/account/login").await[0];
    assert_eq!(
        header(login, "accesstoken"),
        None,
        "login is unauthenticated"
    );
}

#[tokio::test]
async fn wrong_password_is_an_api_error_and_not_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/account/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"result": "1030", "message": "Wrong password"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = CorosClient::new(config(&server)).unwrap();
    let err = client.login(&credentials()).await.unwrap_err();
    assert_eq!(err.result_code(), Some("1030"));
    assert!(client.session().await.is_none());
}

#[tokio::test]
async fn authenticated_requests_send_token_and_yfheader() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/account/query", "account/synthetic.json").await;
    signed_in(&server).account().await.unwrap();
    let r = &requests_to(&server, "/account/query").await[0];
    assert_eq!(header(r, "accesstoken").as_deref(), Some(TOKEN));
    assert_eq!(
        header(r, "yfheader").as_deref(),
        Some(r#"{"userId":"123456789012"}"#)
    );
    assert_eq!(
        header(r, "content-type").as_deref(),
        Some("application/json")
    );
    assert!(header(r, "user-agent").unwrap().starts_with("Mozilla/5.0"));
}

#[tokio::test]
async fn no_session_means_no_request() {
    let server = MockServer::start().await;
    let client = CorosClient::new(config(&server)).unwrap();
    assert!(matches!(
        client.account().await,
        Err(CorosError::NotAuthenticated)
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn detail_is_post_with_query_params_and_empty_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/activity/detail/query"))
        .and(query_param("labelId", "42"))
        .and(query_param("sportType", "102"))
        .and(body_json(serde_json::json!({})))
        .respond_with(json_response("activity_detail/synthetic_run.json"))
        .expect(1)
        .mount(&server)
        .await;
    signed_in(&server)
        .activity_detail(&LabelId("42".into()), SportType::TrailRun)
        .await
        .unwrap();
}

#[tokio::test]
async fn analyse_is_a_get() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/analyse/query"))
        .respond_with(json_response("analyse/synthetic.json"))
        .expect(1)
        .mount(&server)
        .await;
    signed_in(&server).analyse(None).await.unwrap();
}

/// Old token → 1019; new token → success.
async fn mount_token_rotation(server: &MockServer) {
    Mock::given(path("/account/query"))
        .and(header_is("accesstoken", TOKEN))
        .respond_with(json_response("errors/token_invalid.json"))
        .mount(server)
        .await;
    Mock::given(path("/account/query"))
        .and(header_is("accesstoken", "SYNTHETIC-TOKEN-NEW"))
        .respond_with(json_response("account/synthetic.json"))
        .mount(server)
        .await;
    mount(server, "POST", "/account/login", "login/synthetic.json").await;
}

#[tokio::test]
async fn token_rejected_relogs_in_once_and_retries() {
    let server = MockServer::start().await;
    mount_token_rotation(&server).await;
    let saved = Arc::new(AtomicUsize::new(0));
    let saved_in_listener = saved.clone();
    let client = signed_in(&server)
        .with_credentials(credentials())
        .on_session_change(move |_| {
            saved_in_listener.fetch_add(1, Ordering::SeqCst);
        });

    client.account().await.unwrap();
    assert_eq!(requests_to(&server, "/account/login").await.len(), 1);
    assert_eq!(requests_to(&server, "/account/query").await.len(), 2);
    assert_eq!(
        saved.load(Ordering::SeqCst),
        1,
        "new token handed to the caller to persist"
    );

    // The next request uses the new token straight away.
    client.account().await.unwrap();
    assert_eq!(requests_to(&server, "/account/login").await.len(), 1);
}

#[tokio::test]
async fn concurrent_rejections_relogin_only_once() {
    let server = MockServer::start().await;
    mount_token_rotation(&server).await;
    let client = Arc::new(signed_in(&server).with_credentials(credentials()));
    let (a, b) = tokio::join!(client.account(), client.account());
    a.unwrap();
    b.unwrap();
    assert_eq!(requests_to(&server, "/account/login").await.len(), 1);
}

#[tokio::test]
async fn token_still_rejected_after_relogin_gives_up() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(json_response("errors/token_invalid.json"))
        .mount(&server)
        .await;
    mount(&server, "POST", "/account/login", "login/synthetic.json").await;
    let client = signed_in(&server).with_credentials(credentials());
    let err = client.account().await.unwrap_err();
    assert!(err.is_token_invalid());
    assert_eq!(
        requests_to(&server, "/account/login").await.len(),
        1,
        "no login loop"
    );
    assert_eq!(requests_to(&server, "/account/query").await.len(), 2);
}

#[tokio::test]
async fn token_rejected_without_credentials_fails_fast() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(json_response("errors/token_invalid.json"))
        .mount(&server)
        .await;
    let err = signed_in(&server).account().await.unwrap_err();
    assert!(err.is_token_invalid());
    assert!(err.to_string().contains("asia"), "{err}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn writes_are_never_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/training/program/add"))
        .respond_with(json_response("errors/token_invalid.json"))
        .mount(&server)
        .await;
    mount(&server, "POST", "/account/login", "login/synthetic.json").await;
    let client = signed_in(&server).with_credentials(credentials());
    let write = ApiRequest::post("training/program/add", serde_json::json!({"name": "x"}));
    assert!(
        client
            .send_raw(&write)
            .await
            .unwrap_err()
            .is_token_invalid()
    );
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        1,
        "no re-login, no retry"
    );

    let server = MockServer::start().await;
    Mock::given(path("/training/program/add"))
        .respond_with(
            ResponseTemplate::new(429).set_body_string(common::fixture("errors/rate_limited.html")),
        )
        .mount(&server)
        .await;
    let client = signed_in(&server);
    assert!(matches!(
        client.send_raw(&write).await,
        Err(CorosError::RateLimited)
    ));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn rate_limited_reads_back_off_then_succeed() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(
            ResponseTemplate::new(429).set_body_string(common::fixture("errors/rate_limited.html")),
        )
        .up_to_n_times(2)
        .mount(&server)
        .await;
    mount(&server, "GET", "/account/query", "account/synthetic.json").await;
    signed_in(&server).account().await.unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn rate_limited_reads_give_up_after_max_retries() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(
            ResponseTemplate::new(429).set_body_string(common::fixture("errors/rate_limited.html")),
        )
        .mount(&server)
        .await;
    assert!(matches!(
        signed_in(&server).account().await,
        Err(CorosError::RateLimited)
    ));
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        4,
        "1 try + 3 retries"
    );
}

#[tokio::test]
async fn gateway_errors_are_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>Bad Gateway</html>"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount(&server, "GET", "/account/query", "account/synthetic.json").await;
    signed_in(&server).account().await.unwrap();
}

#[tokio::test]
async fn api_errors_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(json_response("errors/service_exception.json"))
        .mount(&server)
        .await;
    let err = signed_in(&server).account().await.unwrap_err();
    match &err {
        CorosError::Api {
            result, tlog_id, ..
        } => {
            assert_eq!(result, "1001");
            assert_eq!(tlog_id.as_deref(), Some("trace-123"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn success_without_data_is_unexpected_for_typed_calls() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/account/query", "generic/ok_no_data.json").await;
    assert!(matches!(
        signed_in(&server).account().await,
        Err(CorosError::Unexpected { .. })
    ));
}

#[tokio::test]
async fn pagination_stops_on_empty_page_even_if_total_is_wrong() {
    let server = MockServer::start().await;
    let page = |n: u32, items: usize| {
        let list: Vec<_> = (0..items)
            .map(|i| serde_json::json!({"labelId": format!("{n}-{i}"), "sportType": 100, "distance": 1000, "totalTime": 300}))
            .collect();
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"result": "0000", "data": {"count": 999, "totalPage": 99, "pageNumber": n, "dataList": list}}))
    };
    Mock::given(path("/activity/query"))
        .and(query_param("pageNumber", "1"))
        .respond_with(page(1, 2))
        .mount(&server)
        .await;
    Mock::given(path("/activity/query"))
        .and(query_param("pageNumber", "2"))
        .respond_with(page(2, 1))
        .mount(&server)
        .await;
    Mock::given(path("/activity/query"))
        .and(query_param("pageNumber", "3"))
        .respond_with(page(3, 0))
        .mount(&server)
        .await;

    let client = signed_in(&server);
    let all = client
        .all_activities(&ActivityQuery::default(), None)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(requests_to(&server, "/activity/query").await.len(), 3);

    let limited = client
        .all_activities(&ActivityQuery::default(), Some(2))
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);
}

#[tokio::test]
async fn invalid_queries_never_reach_the_server() {
    let server = MockServer::start().await;
    let query = ActivityQuery {
        size: 500,
        ..Default::default()
    };
    assert!(matches!(
        signed_in(&server).activities(&query).await,
        Err(CorosError::InvalidInput(_))
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn day_detail_is_fetched_in_chunks_and_merged() {
    let server = MockServer::start().await;
    Mock::given(path("/analyse/dayDetail/query"))
        .and(query_param("startDay", "20260701"))
        .respond_with(json_response("analyse_day_detail/synthetic_a.json"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/analyse/dayDetail/query"))
        .and(query_param("startDay", "20260923"))
        .respond_with(json_response("analyse_day_detail/synthetic_b.json"))
        .expect(1)
        .mount(&server)
        .await;
    let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
    let range = DateRange::new(d(7, 1), d(10, 18)).unwrap(); // 110 days → 84 + 26
    let daily = signed_in(&server).day_detail(range).await.unwrap();
    let dates: Vec<_> = daily.days.iter().map(|m| m.date).collect();
    assert_eq!(
        dates,
        vec![d(10, 16), d(10, 17), d(10, 18)],
        "sorted and de-duplicated"
    );
}

#[tokio::test]
async fn concurrency_is_limited() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(json_response("account/synthetic.json").set_delay(Duration::from_millis(300)))
        .mount(&server)
        .await;
    let client = CorosClient::new(ClientConfig {
        max_concurrent: 1,
        ..config(&server)
    })
    .unwrap()
    .with_session(session());
    let started = Instant::now();
    let (a, b) = tokio::join!(client.account(), client.account());
    a.unwrap();
    b.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(590),
        "requests ran in parallel: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn timeouts_surface_as_network_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/account/query"))
        .respond_with(json_response("account/synthetic.json").set_delay(Duration::from_secs(2)))
        .mount(&server)
        .await;
    let client = CorosClient::new(ClientConfig {
        timeout: Duration::from_millis(100),
        max_retries: 0,
        ..config(&server)
    })
    .unwrap()
    .with_session(session());
    match client.account().await {
        Err(CorosError::Network { source, .. }) => assert!(source.is_timeout()),
        other => panic!("expected timeout, got {other:?}"),
    }
}

#[tokio::test]
async fn raw_requests_return_untyped_data() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/dashboard/query",
        "dashboard/synthetic.json",
    )
    .await;
    let raw = signed_in(&server)
        .send_raw(&coros_api::requests::dashboard())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(raw["summaryInfo"]["sleepHrvData"]["avgSleepHrv"], 52);
}
