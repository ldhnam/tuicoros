//! Every fixture, sent through the real client, decoded into model types.
//! Explicit asserts pin the unit conversions; JSON snapshots catch everything else.
#![allow(clippy::unwrap_used, clippy::expect_used)] // panicking is how tests fail

mod common;

use chrono::NaiveDate;
use common::*;
use coros_api::{
    ActivityQuery, DateRange, FileFormat, LabelId, PerceivedExertion, SportType, StepTarget,
};
use insta::assert_json_snapshot;
use wiremock::MockServer;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn account() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/account/query", "account/synthetic.json").await;
    let account = signed_in(&server).account().await.unwrap();
    assert_eq!(account.user_id, "123456789012"); // numeric in JSON, string here
    assert_eq!(account.birthday, Some(date(1990, 1, 1)));
    assert_json_snapshot!(account);
}

#[tokio::test]
async fn activity_list_mixed_sports() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/activity/query",
        "activity_query/synthetic_mixed.json",
    )
    .await;
    let page = signed_in(&server)
        .activities(&ActivityQuery::default())
        .await
        .unwrap();

    assert_eq!(page.items.len(), 3, "the row without labelId is skipped");
    assert_eq!((page.page, page.total_pages, page.total_count), (1, 1, 3));

    let run = &page.items[0];
    assert_eq!(run.sport, SportType::Run);
    assert_eq!(run.distance.unwrap().0, 10020.5);
    assert_eq!(run.start.unwrap().to_rfc3339(), "2026-10-18T06:42:00+07:00");
    assert_eq!(run.moving.unwrap().as_secs(), 3131);
    let pace = run.avg_pace.unwrap().secs_per_km();
    assert!(
        (pace - 3131.0 / 10.0205).abs() < 0.01,
        "pace from moving time: {pace}"
    );
    assert!((run.calories_kcal.unwrap() - 612.345).abs() < 1e-9);
    assert_eq!(run.normalized_power, None, "np = 0 means absent");

    let bike = &page.items[1];
    assert_eq!(
        bike.id,
        LabelId("476100000000000002".into()),
        "numeric labelId kept as string"
    );
    assert_eq!(
        (bike.ascent, bike.calories_kcal, bike.max_hr),
        (None, None, None)
    );

    let other = &page.items[2];
    assert_eq!(other.sport, SportType::Unknown(4242));
    assert_eq!(
        (other.distance, other.avg_pace, other.end),
        (None, None, None)
    );
    assert_eq!(other.calories_kcal, Some(250.0), "numeric string accepted");

    assert_json_snapshot!(page);
}

#[tokio::test]
async fn activity_list_empty() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/activity/query",
        "activity_query/synthetic_empty.json",
    )
    .await;
    let page = signed_in(&server)
        .activities(&ActivityQuery::default())
        .await
        .unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.total_pages, 0);
}

#[tokio::test]
async fn activity_detail_outdoor_run() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/activity/detail/query",
        "activity_detail/synthetic_run.json",
    )
    .await;
    let detail = signed_in(&server)
        .activity_detail(&LabelId("476100000000000001".into()), SportType::Run)
        .await
        .unwrap();

    let s = &detail.summary;
    assert_eq!(s.distance.unwrap().0, 10020.5, "centi-scaled 1002050");
    assert_eq!(s.moving.unwrap().as_secs_f64(), 3131.0);
    assert_eq!(s.start.unwrap().to_rfc3339(), "2026-10-18T06:42:00+07:00");
    assert_eq!(s.end.unwrap().timestamp_subsec_millis(), 130);
    assert_eq!(s.max_power, Some(401));

    assert_eq!(detail.lap_groups.len(), 2);
    let laps = &detail.lap_groups[0];
    assert_eq!(laps.fastest, vec![1]);
    assert_eq!(laps.laps.len(), 3);
    assert_eq!(laps.laps[0].distance.unwrap().0, 1000.0);
    assert_eq!(laps.laps[1].pace.unwrap().secs_per_km(), 298.0);
    assert_eq!(laps.laps[2].avg_hr, None);
    assert_eq!(laps.laps[2].pace, None, "20.5 m is too short for a pace");

    assert_eq!(detail.samples.len(), 3);
    assert!((detail.samples[0].lat.unwrap() - 47.4).abs() < 1e-9);
    assert_eq!(detail.samples[0].other.get("heart"), Some(&120.0));
    assert_eq!(detail.samples[2].lat, None, "0 = no GPS fix");
    assert!(
        !detail.samples[2].other.contains_key("note"),
        "non-numeric extras dropped"
    );

    assert_eq!(
        detail.feel.perceived_exertion,
        Some(PerceivedExertion::Moderate)
    );
    assert_eq!(detail.feel.note.as_deref(), Some("Felt good"));
    assert!(detail.weather.is_some());

    assert_json_snapshot!(detail);
}

#[tokio::test]
async fn activity_detail_treadmill_without_gps_or_rating() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/activity/detail/query",
        "activity_detail/synthetic_treadmill.json",
    )
    .await;
    let detail = signed_in(&server)
        .activity_detail(&LabelId("1".into()), SportType::IndoorRun)
        .await
        .unwrap();
    assert_eq!(detail.summary.sport, Some(SportType::IndoorRun));
    assert_eq!(detail.summary.avg_pace.unwrap().secs_per_km(), 360.0);
    assert!(detail.samples.is_empty() && detail.lap_groups.is_empty());
    assert_eq!(
        detail.feel.perceived_exertion, None,
        "feelType 0 = not rated"
    );
    assert_eq!(detail.feel.note, None, "empty note = none");
    assert_eq!(detail.weather, None);
}

#[tokio::test]
async fn analyse() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/analyse/query", "analyse/synthetic.json").await;
    let a = signed_in(&server).analyse(None).await.unwrap();
    assert_eq!(a.model_valid, Some(true));
    assert_eq!(a.days.len(), 2, "happenDay 0 dropped");
    let day = &a.days[0];
    assert_eq!((day.rhr, day.test_rhr), (Some(48), Some(46)));
    assert_eq!(day.base_fitness, Some(62.4));
    let empty = &a.days[1];
    assert_eq!(
        (empty.rhr, empty.performance, empty.base_fitness),
        (None, None, None)
    );
    assert_eq!(a.rolling_7d[0].vo2max, Some(53.0));
    assert_eq!(a.weeks[0].week_start, date(2026, 10, 12));
    assert_json_snapshot!(a);
}

#[tokio::test]
async fn analyse_new_account() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/analyse/query",
        "analyse/synthetic_new_account.json",
    )
    .await;
    let a = signed_in(&server).analyse(None).await.unwrap();
    assert_eq!(a.model_valid, Some(false));
    assert!(a.days.is_empty());
}

#[tokio::test]
async fn dashboard_hrv() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/dashboard/query",
        "dashboard/synthetic.json",
    )
    .await;
    let d = signed_in(&server).dashboard().await.unwrap();
    let hrv = d.hrv.unwrap();
    assert_eq!(hrv.latest.as_ref().unwrap().avg, Some(52.0));
    assert_eq!(
        hrv.nights.iter().map(|n| n.date).collect::<Vec<_>>(),
        vec![date(2026, 10, 16), date(2026, 10, 17)]
    );
    assert_eq!(hrv.nights[0].avg, None);
    assert_json_snapshot!(hrv);

    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/dashboard/query",
        "dashboard/synthetic_no_hrv.json",
    )
    .await;
    assert_eq!(signed_in(&server).dashboard().await.unwrap().hrv, None);
}

#[tokio::test]
async fn schedule_with_plan() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/training/schedule/query",
        "schedule/synthetic_plan.json",
    )
    .await;
    let range = DateRange::new(date(2026, 10, 12), date(2026, 10, 18)).unwrap();
    let s = signed_in(&server).schedule(range).await.unwrap();
    assert_eq!(s.plan_name.as_deref(), Some("10K plan"));
    let dates: Vec<_> = s.entries.iter().map(|e| e.date).collect();
    assert_eq!(
        dates,
        vec![date(2026, 10, 13), date(2026, 10, 17), date(2026, 10, 19)],
        "sorted, bad date dropped"
    );
    assert_eq!(
        s.entries[0].workout.as_ref().unwrap().name.as_deref(),
        Some("Easy 8k"),
        "numeric idInPlan matches string"
    );
    assert_eq!(
        s.entries[1]
            .workout
            .as_ref()
            .unwrap()
            .duration
            .unwrap()
            .as_secs(),
        3300
    );
    assert_eq!(s.entries[2].workout, None, "unknown idInPlan");
    assert_eq!(
        s.weeks[0].plan_distance_raw,
        Some(48000.0),
        "string number parsed"
    );
    assert_json_snapshot!(s);
}

#[tokio::test]
async fn schedule_without_plan() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/training/schedule/query",
        "schedule/synthetic_no_plan.json",
    )
    .await;
    let range = DateRange::new(date(2026, 10, 12), date(2026, 10, 18)).unwrap();
    let s = signed_in(&server).schedule(range).await.unwrap();
    assert!(s.entries.is_empty() && s.weeks.is_empty() && s.plan_name.is_none());
}

#[tokio::test]
async fn workouts() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/training/program/query",
        "workouts/synthetic.json",
    )
    .await;
    let w = signed_in(&server).workouts().await.unwrap();
    assert_eq!(w.len(), 1, "workout without id skipped");
    let steps = &w[0].steps;
    assert_eq!(steps[0].target, StepTarget::Time { seconds: 900.0 });
    assert_eq!(
        steps[1].target,
        StepTarget::Distance {
            meters: coros_api::Meters(800.0)
        }
    );
    assert_eq!(
        (
            steps[1].sets,
            steps[1].intensity_low,
            steps[1].intensity_high
        ),
        (6, Some(225.0), Some(235.0))
    );
    assert_eq!(steps[2].target, StepTarget::Open);
    assert_eq!(steps[3].sets, 1, "missing sets defaults to 1");
    assert!(matches!(
        steps[4].target,
        StepTarget::Other { target_type: 9, .. }
    ));
    assert_json_snapshot!(w);
}

#[tokio::test]
async fn download_url_and_file() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/activity/detail/download",
        "activity_download/synthetic.json",
    )
    .await;
    wiremock::Mock::given(wiremock::matchers::path("/files/476100000000000001.fit"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(b"FITDATA".to_vec()))
        .mount(&server)
        .await;
    let client = signed_in(&server);
    let id = LabelId("476100000000000001".into());
    let url = client
        .activity_file_url(&id, SportType::Run, FileFormat::Fit)
        .await
        .unwrap();
    assert!(url.ends_with("sig=SYNTHETIC"));
    // The fixture's URL is relative; make it absolute against the mock server.
    let bytes = client
        .download(&format!("{}{}", server.uri(), url))
        .await
        .unwrap();
    assert_eq!(bytes, b"FITDATA");
    let file_request = &requests_to(&server, "/files/476100000000000001.fit").await[0];
    assert_eq!(
        header(file_request, "accesstoken"),
        None,
        "token must not go to the file host"
    );
}
