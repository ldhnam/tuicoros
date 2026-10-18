//! Unofficial client for the private COROS Training Hub API (the backend behind
//! <https://t.coros.com>).
//!
//! This crate must stay free of terminal/UI dependencies. See
//! `docs/COROS_API.md` for what is known about the API.
//!
//! Layout: each API area (account, activity, analyse, dashboard, training) is
//! one module holding its request builders, its private `Raw*` JSON mirrors and
//! its public, unit-correct model types. Raw types never leave their module, so
//! all unit conversion happens exactly once (ADR-005).

mod account;
mod activity;
mod analyse;
mod auth;
mod client;
mod dashboard;
mod envelope;
mod error;
mod region;
mod request;
mod sport;
mod training;
mod units;
mod util;

pub use account::Account;
pub use activity::{
    ActivityDetail, ActivityQuery, ActivitySummary, DetailSummary, Feel, FileFormat, HrZone, Lap,
    LapGroup, PerceivedExertion, Sample, Weather,
};
pub use analyse::{Analyse, Daily, DailyMetrics, WeekLoad};
pub use auth::{Credentials, Session, md5_hex};
pub use client::{ClientConfig, CorosClient, DEFAULT_USER_AGENT};
pub use dashboard::{AbilityScores, Dashboard, HrvNight, HrvSummary};
pub use error::{CorosError, Result};
pub use region::{ParseRegionError, Region};
pub use request::{ApiRequest, HttpMethod};
pub use sport::{SportCategory, SportType};
pub use training::{
    Intensity, PlannedWorkout, Schedule, ScheduledWorkout, StepKind, StepTarget, WeekPlan, Workout,
    WorkoutStep,
};
pub use units::{DateRange, LabelId, Meters, Pace, Page, yyyymmdd};

/// Request builders for every endpoint, for callers that want the raw JSON
/// (`CorosClient::send_raw`), e.g. `tuicoros api … --raw`.
pub mod requests {
    pub use crate::account::account_request as account;
    pub use crate::activity::{
        activities_request as activities, activity_detail_request as activity_detail,
        activity_download_request as activity_download,
    };
    pub use crate::analyse::{analyse_request as analyse, day_detail_request as day_detail};
    pub use crate::dashboard::dashboard_request as dashboard;
    pub use crate::training::{schedule_request as schedule, workouts_request as workouts};
}
