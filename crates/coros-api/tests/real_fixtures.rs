//! Tests against **real** COROS responses (captured 2026-10-18 from the
//! owner's account, scrubbed with `xtask scrub`). These pin what was verified
//! live; see docs/COROS_API.md §9.
#![allow(clippy::unwrap_used, clippy::expect_used)] // panicking is how tests fail

mod common;

use std::collections::HashSet;

use chrono::NaiveDate;
use common::*;
use coros_api::{
    ActivityDetail, ActivityQuery, DateRange, Intensity, LabelId, LapGroup, Meters, Pace,
    SportType, StepKind, StepTarget,
};
use wiremock::MockServer;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

async fn detail(fixture: &str, sport: SportType) -> ActivityDetail {
    let server = MockServer::start().await;
    mount(&server, "POST", "/activity/detail/query", fixture).await;
    signed_in(&server)
        .activity_detail(&LabelId("1".into()), sport)
        .await
        .unwrap()
}

#[tokio::test]
async fn activity_list() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/activity/query",
        "activity_query/real_90_days.json",
    )
    .await;
    let page = signed_in(&server)
        .activities(&ActivityQuery::default())
        .await
        .unwrap();
    assert_eq!(page.items.len(), 50);
    let sports: HashSet<_> = page.items.iter().map(|a| a.sport).collect();
    for s in [
        SportType::Run,
        SportType::TrailRun,
        SportType::IndoorRun,
        SportType::Strength,
    ] {
        assert!(sports.contains(&s), "missing {s}");
    }
    for a in &page.items {
        let start = a.start.unwrap();
        assert_eq!(
            start.offset().local_minus_utc(),
            7 * 3600,
            "UTC+7 (quarter-hours = 28)"
        );
        assert_eq!(
            Some(start.date_naive()),
            a.date,
            "local date matches start time"
        );
        match a.sport {
            SportType::Strength => assert_eq!((a.distance, a.avg_pace), (None, None)),
            _ => {
                let km = a.distance.unwrap().km();
                let pace = a.avg_pace.unwrap().secs_per_km();
                assert!((1.0..60.0).contains(&km), "{km} km");
                assert!((180.0..1200.0).contains(&pace), "{pace} s/km");
            }
        }
    }
}

#[tokio::test]
async fn steady_run_12k() {
    let d = detail("activity_detail/real_run_12k.json", SportType::Run).await;
    let s = &d.summary;
    assert_eq!(s.distance, Some(Meters(12002.81)));
    assert_eq!(s.moving.unwrap().as_secs_f64(), 3830.77);
    assert_eq!(s.best_km, Some(Pace(284.0)));
    assert_eq!(
        (s.elevation_gain, s.elevation_loss),
        (Some(Meters(17.0)), Some(Meters(19.0)))
    );
    assert_eq!(
        (s.max_elevation, s.min_elevation),
        (None, None),
        "0 = no altitude range"
    );
    assert_eq!(
        (s.aerobic_effect, s.anaerobic_effect, s.vo2max),
        (Some(4.0), Some(3.0), Some(55.0))
    );

    let labels: Vec<_> = d.lap_groups.iter().map(LapGroup::label).collect();
    assert_eq!(
        labels,
        [
            "5 km segments",
            "10 km segments",
            "1 km laps",
            "Whole activity"
        ]
    );
    let laps = LapGroup::preferred(&d.lap_groups).unwrap();
    assert_eq!(laps.label(), "1 km laps");
    assert_eq!(laps.laps.len(), 12);
    let total: f64 = laps.laps.iter().map(|l| l.distance.unwrap().0).sum();
    // Auto laps are stored as exactly 1000 m, so they sum to 12000, not 12002.81.
    assert!((total - 12002.81).abs() < 5.0, "laps add up: {total}");

    assert_eq!(d.hr_zones.len(), 6);
    assert_eq!(d.hr_zones[3].time.unwrap().as_secs(), 2485);
    assert_eq!(
        (d.hr_zones[3].low_bpm, d.hr_zones[3].high_bpm),
        (Some(166), Some(177))
    );

    assert!(
        !d.samples.is_empty() && d.samples.len() <= 200,
        "thinned by the scrubber"
    );
    assert!(d.samples.iter().filter(|x| x.heart_rate.is_some()).count() > d.samples.len() / 2);
    assert!(d.samples.iter().any(|x| x.other.contains_key("groundTime")));
}

#[tokio::test]
async fn interval_run_steps() {
    let d = detail("activity_detail/real_interval.json", SportType::Run).await;
    let steps = LapGroup::preferred(&d.lap_groups).unwrap();
    assert_eq!(steps.label(), "Workout steps");
    assert_eq!(steps.laps.len(), 16);
    // The plan was 8 × 700 m @ 4:20–4:30 /km with 1:30 recoveries; the last rep
    // was cut short when the watch was stopped.
    for rep in steps
        .laps
        .iter()
        .filter(|l| l.step_kind == Some(StepKind::Training))
        .take(7)
    {
        let m = rep.distance.expect("derived from pace").0;
        let p = rep.pace.unwrap().secs_per_km();
        assert!((670.0..730.0).contains(&m), "rep {} = {m} m", rep.index);
        assert!((255.0..275.0).contains(&p), "rep {} = {p} s/km", rep.index);
    }
    let recoveries = steps
        .laps
        .iter()
        .filter(|l| l.step_kind == Some(StepKind::Recovery))
        .count();
    assert_eq!(recoveries, 8);
}

#[tokio::test]
async fn trail_run_elevation() {
    let d = detail("activity_detail/real_trail.json", SportType::TrailRun).await;
    let s = &d.summary;
    assert_eq!(s.sport, Some(SportType::TrailRun));
    assert_eq!(
        (s.elevation_gain, s.elevation_loss),
        (Some(Meters(511.0)), Some(Meters(528.0)))
    );
    assert_eq!(
        (s.min_elevation, s.max_elevation),
        (Some(58.0), Some(487.0))
    );
}

#[tokio::test]
async fn strength_session() {
    let d = detail("activity_detail/real_strength.json", SportType::Strength).await;
    assert_eq!((d.summary.distance, d.summary.avg_pace), (None, None));
    assert_eq!(d.lap_groups[0].laps.len(), 38, "one lap per set");
}

#[tokio::test]
async fn indoor_run_has_no_gps() {
    let d = detail("activity_detail/real_indoor.json", SportType::IndoorRun).await;
    assert_eq!(d.summary.sport, Some(SportType::IndoorRun));
    assert!(d.samples.iter().all(|x| x.lat.is_none()));
    assert!(d.summary.distance.is_some());
}

#[tokio::test]
async fn evolab_relationships() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/analyse/query", "analyse/real.json").await;
    let a = signed_in(&server).analyse(None).await.unwrap();
    assert_eq!(a.model_valid, Some(true));
    assert!(a.days.len() > 50);
    for d in &a.days {
        if let (Some(ati), Some(cti)) = (d.acute_load, d.chronic_load) {
            // fatigue = acute − chronic (0 comes back as "absent")
            assert!(
                (d.fatigue.unwrap_or(0.0) - (ati - cti)).abs() <= 1.0,
                "{}",
                d.date
            );
            assert!(
                (d.load_ratio.unwrap() - ati / cti).abs() <= 0.02,
                "{}",
                d.date
            );
        }
    }
    let last = a.days.last().unwrap();
    assert_eq!(last.date, date(2026, 10, 18));
    assert_eq!(
        (last.lthr, last.lt_pace, last.vo2max),
        (Some(174), Some(Pace(280.0)), Some(55.0))
    );
}

#[tokio::test]
async fn day_detail() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/analyse/dayDetail/query",
        "analyse_day_detail/real.json",
    )
    .await;
    let range = DateRange::new(date(2026, 10, 4), date(2026, 10, 18)).unwrap();
    let days = signed_in(&server).day_detail(range).await.unwrap();
    assert!(!days.is_empty());
    assert!(days.windows(2).all(|w| w[0].date < w[1].date));
}

#[tokio::test]
async fn dashboard() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/dashboard/query", "dashboard/real.json").await;
    let d = signed_in(&server).dashboard().await.unwrap();
    assert_eq!(
        (d.lthr, d.lt_pace, d.max_hr),
        (Some(174), Some(Pace(280.0)), Some(195))
    );
    assert!(d.recovery_pct.is_some_and(|r| (0.0..=100.0).contains(&r)));
    assert!(d.scores.aerobic_endurance.is_some());
    let hrv = d.hrv.unwrap();
    assert_eq!(
        hrv.remaining_wear_days,
        Some(1),
        "not enough nights worn yet"
    );
    assert!(hrv.nights.is_empty() && hrv.latest.is_none());
}

#[tokio::test]
async fn schedule_links_plan_to_activities() {
    let server = MockServer::start().await;
    mount(
        &server,
        "GET",
        "/training/schedule/query",
        "schedule/real.json",
    )
    .await;
    mount(
        &server,
        "GET",
        "/activity/query",
        "activity_query/real_90_days.json",
    )
    .await;
    let client = signed_in(&server);
    let range = DateRange::new(date(2026, 10, 12), date(2026, 11, 8)).unwrap();
    let s = client.schedule(range).await.unwrap();

    let week = s
        .weeks
        .iter()
        .find(|w| w.week_start == date(2026, 10, 12))
        .unwrap();
    assert_eq!(
        week.actual_distance,
        Some(Meters(32650.15)),
        "\"3265015.00\" = 32.65 km"
    );
    assert_eq!(week.plan_distance, Some(Meters(12600.0)));
    assert_eq!(
        (week.plan_load, week.actual_load),
        (Some(279.0), Some(678.0))
    );

    let done: Vec<_> = s
        .entries
        .iter()
        .filter(|e| e.date >= date(2026, 10, 12))
        .collect();
    assert_eq!(done.len(), 2);
    let interval = done
        .iter()
        .find(|e| e.workout.as_ref().unwrap().name.as_deref() == Some("Interval"))
        .unwrap();
    assert_eq!(
        interval.workout.as_ref().unwrap().distance,
        Some(Meters(5600.0)),
        "8 × 700 m"
    );
    assert_eq!(interval.completion, Some(0.94));

    // Every linked activity exists in the activity list (ids are scrubbed consistently).
    let list = client.activities(&ActivityQuery::default()).await.unwrap();
    let ids: HashSet<_> = list.items.iter().map(|a| a.id.clone()).collect();
    for e in done {
        assert!(
            ids.contains(e.activity_id.as_ref().unwrap()),
            "{:?}",
            e.activity_id
        );
    }
}

#[tokio::test]
async fn workout_library_structure() {
    let server = MockServer::start().await;
    mount(
        &server,
        "POST",
        "/training/program/query",
        "workouts/real.json",
    )
    .await;
    let w = signed_in(&server).workouts().await.unwrap();
    assert_eq!(w.len(), 2);

    let interval = w
        .iter()
        .find(|w| w.name.as_deref() == Some("Interval"))
        .unwrap();
    assert_eq!(interval.distance, Some(Meters(5600.0)));
    assert_eq!(interval.steps.len(), 1);
    let group = &interval.steps[0];
    assert_eq!(
        (group.kind, group.sets, group.label.as_str()),
        (Some(StepKind::Group), 8, "Repeat")
    );
    let (run, rest) = (&group.steps[0], &group.steps[1]);
    assert_eq!(run.label, "Run");
    assert_eq!(
        run.target,
        StepTarget::Distance {
            meters: Meters(700.0)
        }
    );
    assert_eq!(
        run.intensity,
        Some(Intensity::Pace {
            fastest: Pace(260.0),
            slowest: Pace(270.0)
        })
    );
    assert_eq!(
        (rest.kind, rest.target.clone()),
        (Some(StepKind::Recovery), StepTarget::Time { seconds: 90.0 })
    );

    let progressive = w
        .iter()
        .find(|w| w.name.as_deref() == Some("Progressive Run"))
        .unwrap();
    assert_eq!(progressive.distance, Some(Meters(7000.0)));
    assert_eq!(progressive.steps.len(), 2, "a 3× group, then a final 1 km");
    assert_eq!(progressive.steps[0].sets, 3);
    assert_eq!(
        progressive.steps[1].target,
        StepTarget::Distance {
            meters: Meters(1000.0)
        }
    );
}
