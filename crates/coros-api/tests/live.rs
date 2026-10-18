//! Read-only smoke tests against the real COROS API. Never run in CI.
//!
//! ```sh
//! COROS_LIVE=1 COROS_EMAIL=… COROS_PASSWORD=… COROS_REGION=asia \
//!   cargo test -p coros-api --test live -- --ignored --test-threads=1
//! ```
//!
//! Each test logs in once (COROS rate-limits logins, so keep runs rare), then
//! checks that responses decode into sane model values. Their job is to catch
//! API drift, not to test our logic.
#![allow(clippy::unwrap_used, clippy::expect_used)] // panicking is how tests fail

use std::time::Duration;

use chrono::{Local, NaiveDate};
use coros_api::{ActivityQuery, ClientConfig, CorosClient, Credentials, DateRange, Region};

async fn live_client() -> Option<CorosClient> {
    if std::env::var("COROS_LIVE").ok().as_deref() != Some("1") {
        eprintln!("skipping: set COROS_LIVE=1 (plus COROS_EMAIL, COROS_PASSWORD, COROS_REGION)");
        return None;
    }
    let email = std::env::var("COROS_EMAIL").expect("COROS_EMAIL");
    let password = std::env::var("COROS_PASSWORD").expect("COROS_PASSWORD");
    let region: Region = std::env::var("COROS_REGION")
        .unwrap_or_else(|_| "asia".into())
        .parse()
        .expect("COROS_REGION");
    let credentials = Credentials::from_password(email, &password);
    let client = CorosClient::new(ClientConfig::new(region))
        .unwrap()
        .with_credentials(credentials.clone());
    client.login(&credentials).await.expect("login");
    Some(client)
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

#[tokio::test]
#[ignore = "talks to the real COROS API"]
async fn live_read_only_endpoints() {
    let Some(client) = live_client().await else {
        return;
    };
    let pause = || tokio::time::sleep(Duration::from_millis(500));

    let account = client.account().await.expect("account");
    assert!(!account.user_id.is_empty());
    pause().await;

    let range = DateRange::last_days(today(), 90);
    let query = ActivityQuery {
        size: 20,
        from: Some(range.start),
        to: Some(range.end),
        ..Default::default()
    };
    let page = client.activities(&query).await.expect("activities");
    for a in &page.items {
        assert!(!a.id.0.is_empty());
        if let Some(d) = a.distance {
            assert!(
                d.0 > 0.0 && d.0 < 1_000_000.0,
                "distance {d:?} for {}",
                a.id
            );
        }
        if let (Some(start), Some(date)) = (a.start, a.date) {
            assert_eq!(
                start.date_naive(),
                date,
                "start time and date disagree for {}",
                a.id
            );
        }
    }
    pause().await;

    if let Some(first) = page.items.first() {
        let detail = client
            .activity_detail(&first.id, first.sport)
            .await
            .expect("detail");
        let (list_km, detail_km) = (
            first.distance.map(|d| d.km()),
            detail.summary.distance.map(|d| d.km()),
        );
        if let (Some(a), Some(b)) = (list_km, detail_km) {
            assert!(
                (a - b).abs() < 0.05,
                "list says {a} km, detail says {b} km: scaling bug?"
            );
        }
        pause().await;
    }

    client.analyse(None).await.expect("analyse");
    pause().await;
    client
        .day_detail(DateRange::last_days(today(), 28))
        .await
        .expect("day detail");
    pause().await;
    client.dashboard().await.expect("dashboard");
    pause().await;
    client
        .schedule(DateRange::last_days(
            today() + chrono::Duration::days(21),
            28,
        ))
        .await
        .expect("schedule");
    pause().await;
    client.workouts().await.expect("workouts");
}
