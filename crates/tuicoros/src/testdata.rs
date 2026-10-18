//! Deterministic, made-up data for tests and render snapshots.

use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use coros_api::{
    AbilityScores, ActivityDetail, ActivitySummary, Daily, DailyMetrics, Dashboard, DetailSummary,
    Feel, HrZone, HrvSummary, LabelId, Lap, LapGroup, Meters, Pace, Page, PerceivedExertion,
    Sample, Schedule, ScheduledWorkout, SportType, StepKind, Weather, WeekLoad, WeekPlan,
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
        samples: samples(summary.start.map(|t| t.with_timezone(&Utc))),
        feel: Feel {
            perceived_exertion: Some(PerceivedExertion::Hard),
            note: Some("Windy, legs felt heavy".to_owned()),
        },
        weather: Some(Weather {
            temperature_c: Some(26.1),
            feels_like_c: Some(28.4),
            humidity_pct: Some(81.0),
            wind_speed: Some(6.8),
            wind_direction_deg: Some(338.0),
            condition_code: Some(12),
        }),
    }
}

pub fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// 600 samples of an oval 10 km loop with intervals: HR, pace, altitude, cadence, power.
fn samples(start: Option<DateTime<Utc>>) -> Vec<Sample> {
    let start = start.unwrap_or_else(now);
    (0..600)
        .map(|i| {
            let t = f64::from(i) / 600.0 * std::f64::consts::TAU;
            let rep = (i / 30) % 2 == 0; // alternating 30-sample blocks
            Sample {
                time: Some(start + chrono::Duration::seconds(i64::from(i) * 5)),
                lat: Some(46.50 + 0.012 * t.sin()),
                lon: Some(6.60 + 0.025 * t.cos()),
                distance: Some(Meters(f64::from(i) * 16.7)),
                heart_rate: Some(140 + (i / 20) as u32 + if rep { 12 } else { 0 }),
                cadence: Some(if rep { 184 } else { 168 }),
                power: Some(if rep { 260 } else { 170 }),
                altitude: Some(15.0 + 12.0 * (2.0 * t).sin()),
                pace: Some(Pace(if rep { 265.0 } else { 390.0 })),
                adjusted_pace: None,
                other: Default::default(),
            }
        })
        .collect()
}

pub fn dashboard() -> Dashboard {
    Dashboard {
        recovery_pct: Some(89.0),
        full_recovery_hours: Some(21.0),
        resting_hr: Some(54),
        max_hr: Some(195),
        lthr: Some(174),
        lt_pace: Some(Pace(280.0)),
        stamina_level: Some(80.7),
        scores: AbilityScores {
            aerobic_endurance: Some(80.3),
            lactate_threshold: Some(80.5),
            anaerobic_endurance: Some(80.5),
            anaerobic_capacity: Some(80.7),
        },
        hrv: Some(HrvSummary {
            latest: None,
            nights: Vec::new(),
            baseline: Some(61.0),
            remaining_wear_days: Some(1),
        }),
    }
}

/// `days` days of EvoLab metrics ending today, with weekly loads.
pub fn daily(days: u32) -> Daily {
    let first = today() - chrono::Duration::days(i64::from(days) - 1);
    let days: Vec<DailyMetrics> = (0..days)
        .map(|i| {
            let f = f64::from(i);
            let acute = 90.0 + 15.0 * (f / 4.0).sin() + f * 0.3;
            let chronic = 80.0 + f * 0.25;
            DailyMetrics {
                date: first + chrono::Duration::days(i64::from(i)),
                distance: None,
                duration: None,
                rhr: Some(55 + (i % 5)),
                test_rhr: None,
                training_load: (i % 3 != 2).then_some(100 + (i % 7) * 25),
                load_7d: Some(600.0),
                load_28d: Some(2400.0),
                acute_load: Some(acute),
                chronic_load: Some(chronic),
                load_ratio: Some(acute / chronic),
                load_ratio_state: Some(4),
                fatigue: Some(acute - chronic),
                fatigue_state: Some(4),
                performance: Some(2.0),
                recommended_load_min: Some(672.0),
                recommended_load_max: Some(1008.0),
                avg_sleep_hrv: None,
                sleep_hrv_baseline: None,
                vo2max: Some(54.0 + (f / 20.0).floor()),
                lthr: Some(172 + (i / 20)),
                lt_pace: Some(Pace(290.0 - f / 4.0)),
                stamina_level: Some(80.7),
                stamina_level_7d: Some(100.0),
            }
        })
        .collect();
    let mut weeks = Vec::new();
    let mut monday = crate::screens::dashboard::week_start(first);
    let mut n = 0.0;
    while monday <= today() {
        weeks.push(WeekLoad {
            week_start: monday,
            training_load: Some(600.0 + n * 37.0),
            recommended_min: Some(672.0),
            recommended_max: Some(1008.0),
        });
        monday += chrono::Duration::days(7);
        n += 1.0;
    }
    Daily { days, weeks }
}

/// This week (Mon 12 – Sun 18 Oct): 12.6 km planned, 32.65 km done.
pub fn schedule() -> Schedule {
    Schedule {
        plan_id: Some("plan-1".into()),
        plan_name: Some("10K plan".into()),
        plan_start: Some(date(2026, 10, 12)),
        plan_end: Some(date(2026, 11, 8)),
        entries: vec![ScheduledWorkout {
            date: date(2026, 10, 13),
            id_in_plan: Some("1".into()),
            workout: None,
            activity_id: Some(activity(5).id),
            completion: Some(0.94),
            score: Some(96.2),
        }],
        weeks: vec![WeekPlan {
            week_start: date(2026, 10, 12),
            plan_distance: Some(Meters(12_600.0)),
            actual_distance: Some(Meters(32_650.0)),
            plan_duration: Some(Duration::from_secs(4684)),
            actual_duration: Some(Duration::from_secs(10_908)),
            plan_load: Some(279.0),
            actual_load: Some(678.0),
        }],
    }
}
