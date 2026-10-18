//! Deterministic, made-up data for tests and render snapshots.

use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use coros_api::{
    ActivityDetail, ActivitySummary, DetailSummary, Feel, HrZone, LabelId, Lap, LapGroup, Meters,
    Pace, Page, PerceivedExertion, SportType, StepKind,
};

pub fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 18).unwrap()
}

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 18, 8, 0, 0).unwrap()
}

fn tz() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).unwrap()
}

const KINDS: [(&str, SportType, f64, f64); 6] = [
    ("Morning Run", SportType::Run, 10_020.0, 312.0),
    ("Interval", SportType::Run, 6_400.0, 331.0),
    ("Progressive Run", SportType::Run, 7_250.0, 345.0),
    ("Strength", SportType::Strength, 0.0, 0.0),
    ("Trail Run", SportType::TrailRun, 10_410.0, 845.0),
    ("Treadmill", SportType::IndoorRun, 8_000.0, 361.0),
];

/// Activity number `n` (0 = newest), one per day going back from `today()`.
pub fn activity(n: usize) -> ActivitySummary {
    let (name, sport, base_m, base_pace) = KINDS[n % KINDS.len()];
    let day = today() - chrono::Duration::days(n as i64);
    let start = tz()
        .from_local_datetime(&day.and_hms_opt(6, 0, 0).unwrap())
        .unwrap();
    let distance = (base_m > 0.0).then_some(Meters(base_m + (n % 7) as f64 * 113.0));
    let pace = (base_pace > 0.0).then_some(Pace(base_pace + (n % 5) as f64 * 4.0));
    let moving = match (distance, pace) {
        (Some(d), Some(p)) => Duration::from_secs_f64(d.km() * p.0),
        _ => Duration::from_secs(2700 + n as u64 * 7),
    };
    ActivitySummary {
        id: LabelId(format!("47610000{n:06}")),
        name: Some(name.to_owned()),
        sport,
        date: Some(start.date_naive()),
        start: Some(start),
        end: Some(start + chrono::Duration::from_std(moving).unwrap_or_default()),
        distance,
        elapsed: Some(moving + Duration::from_secs(90)),
        moving: Some(moving),
        avg_pace: pace,
        avg_hr: Some(140 + (n % 30) as u32),
        max_hr: Some(175),
        avg_cadence: distance.map(|_| 176),
        avg_power: distance.map(|_| 210),
        normalized_power: None,
        ascent: distance.map(|_| Meters(20.0)),
        descent: distance.map(|_| Meters(18.0)),
        calories_kcal: Some(500.0),
        training_load: Some(50 + (n % 9) as u32 * 20),
        steps: None,
        device: Some("COROS PACE 4".to_owned()),
    }
}

/// Page `page` of size `n` (activities numbered continuously across pages).
pub fn page(page: u32, n: usize, total_pages: u32) -> Page<ActivitySummary> {
    let first = (page as usize - 1) * n;
    Page {
        items: (first..first + n).map(activity).collect(),
        page,
        total_pages,
        total_count: (total_pages as usize * n) as u32,
    }
}

fn lap(index: u32, meters: f64, secs: f64, hr: u32, kind: Option<StepKind>) -> Lap {
    Lap {
        index,
        step_kind: kind,
        distance: Some(Meters(meters)),
        duration: Some(Duration::from_secs_f64(secs)),
        pace: Pace::from_distance_time(Meters(meters), Duration::from_secs_f64(secs)),
        avg_hr: Some(hr),
        max_hr: Some(hr + 8),
        avg_cadence: Some(178),
        avg_power: Some(210),
        elevation_gain: None,
    }
}

/// A 10 km interval-ish run with auto laps, workout steps and HR zones.
pub fn detail() -> ActivityDetail {
    let summary = activity(1);
    let km_laps: Vec<Lap> = (1..=10)
        .map(|i| lap(i, 1000.0, 300.0 + f64::from(i % 4) * 9.0, 150 + i, None))
        .collect();
    let steps: Vec<Lap> = (1..=8)
        .map(|i| {
            if i % 2 == 1 {
                lap(i, 702.0, 186.0, 165, Some(StepKind::Training))
            } else {
                lap(i, 135.0, 90.0, 145, Some(StepKind::Recovery))
            }
        })
        .collect();
    let zone = |zone, low, high, secs: u64, percent| HrZone {
        zone,
        low_bpm: Some(low),
        high_bpm: Some(high),
        time: Some(Duration::from_secs(secs)),
        percent: Some(percent),
    };
    ActivityDetail {
        id: summary.id.clone(),
        summary: DetailSummary {
            name: summary.name.clone(),
            sport: Some(summary.sport),
            start: summary.start,
            end: summary.end,
            distance: Some(Meters(10_020.0)),
            elapsed: Some(Duration::from_secs(3220)),
            moving: Some(Duration::from_secs(3131)),
            avg_pace: Some(Pace(312.5)),
            best_km: Some(Pace(284.0)),
            adjusted_pace: Some(Pace(316.0)),
            avg_hr: Some(166),
            max_hr: Some(191),
            avg_cadence: Some(178),
            max_cadence: Some(187),
            avg_stride_length_cm: Some(106),
            avg_power: Some(209),
            max_power: Some(257),
            calories_kcal: Some(612.3),
            training_load: Some(170),
            elevation_gain: Some(Meters(27.0)),
            elevation_loss: Some(Meters(25.0)),
            max_elevation: Some(32.0),
            min_elevation: Some(4.0),
            aerobic_effect: Some(2.9),
            anaerobic_effect: Some(4.0),
            vo2max: Some(55.0),
        },
        lap_groups: vec![
            LapGroup {
                kind: Some(2),
                split: Some(Meters(1000.0)),
                fastest: vec![9],
                laps: km_laps,
            },
            LapGroup {
                kind: Some(10),
                split: None,
                fastest: vec![2],
                laps: steps,
            },
            LapGroup {
                kind: Some(-1),
                split: None,
                fastest: vec![],
                laps: vec![lap(1, 10_020.0, 3131.0, 166, None)],
            },
        ],
        hr_zones: vec![
            zone(0, 139, 157, 290, 15.0),
            zone(1, 139, 157, 320, 15.0),
            zone(2, 158, 165, 180, 8.0),
            zone(3, 166, 177, 515, 24.0),
            zone(4, 178, 184, 449, 21.0),
            zone(5, 184, 255, 365, 17.0),
        ],
        samples: Vec::new(),
        feel: Feel {
            perceived_exertion: Some(PerceivedExertion::Hard),
            note: Some("Windy, legs felt heavy".to_owned()),
        },
        weather: None,
    }
}
