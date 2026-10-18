//! Training calendar (`training/schedule/query`) and workout library
//! (`training/program/query`). Read-only.
//!
//! Workouts use their own sport numbering (runs are `1`, not `100`), so they
//! keep the raw `sport_code` rather than a `SportType`.

use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::client::CorosClient;
use crate::error::Result;
use crate::request::ApiRequest;
use crate::units::{DateRange, Meters, date_from_int, duration, ser_opt_secs, yyyymmdd};
use crate::util::{FlexStr, Num};

pub fn schedule_request(range: DateRange) -> ApiRequest {
    ApiRequest::get("training/schedule/query")
        .query("startDate", yyyymmdd(range.start))
        .query("endDate", yyyymmdd(range.end))
        .query("supportRestExercise", "1")
}

pub fn workouts_request() -> ApiRequest {
    ApiRequest::post("training/program/query", serde_json::json!({})).read_only()
}

impl CorosClient {
    /// `GET training/schedule/query`. An account without a plan gets an empty schedule.
    pub async fn schedule(&self, range: DateRange) -> Result<Schedule> {
        let raw: Option<RawSchedule> = self.send(&schedule_request(range)).await?;
        Ok(raw.unwrap_or_default().into_schedule())
    }

    /// `POST training/program/query`: the workout library.
    pub async fn workouts(&self) -> Result<Vec<Workout>> {
        let raw: Vec<RawProgram> = self.send(&workouts_request()).await?;
        Ok(raw
            .into_iter()
            .filter_map(RawProgram::into_workout)
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Schedule {
    pub plan_id: Option<String>,
    pub plan_name: Option<String>,
    pub plan_start: Option<NaiveDate>,
    pub plan_end: Option<NaiveDate>,
    /// Calendar entries, sorted by date.
    pub entries: Vec<ScheduledWorkout>,
    pub weeks: Vec<WeekPlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScheduledWorkout {
    pub date: NaiveDate,
    pub id_in_plan: Option<String>,
    /// The workout definition, when the entry links to one in `programs`.
    pub workout: Option<PlannedWorkout>,
}

/// A workout as it appears on the calendar.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedWorkout {
    pub id: Option<String>,
    pub name: Option<String>,
    pub sport_code: Option<u32>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub duration: Option<Duration>,
    /// As sent by the API; scale to be verified live.
    pub distance_raw: Option<f64>,
    pub training_load: Option<u32>,
    pub exercise_count: Option<u32>,
}

/// Plan vs actual totals for one week. Distances arrive as strings; their unit
/// is to be verified live, so they are kept raw.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeekPlan {
    pub week_start: NaiveDate,
    pub plan_distance_raw: Option<f64>,
    pub actual_distance_raw: Option<f64>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub plan_duration: Option<Duration>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub actual_duration: Option<Duration>,
    pub plan_load: Option<f64>,
    pub actual_load: Option<f64>,
    pub plan_count: Option<u32>,
    pub actual_count: Option<u32>,
}

/// A workout from the library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Workout {
    pub id: String,
    pub name: Option<String>,
    /// Workout-namespace sport code (1 = run, 2 = indoor bike, 4 = strength, 200 = road bike).
    pub sport_code: Option<u32>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub estimated_time: Option<Duration>,
    pub training_load: Option<u32>,
    pub steps: Vec<WorkoutStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WorkoutStep {
    pub name: Option<String>,
    pub sets: u32,
    pub target: StepTarget,
    /// Intensity band as sent by the API (unit depends on the intensity type).
    pub intensity_low: Option<f64>,
    pub intensity_high: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepTarget {
    Distance {
        meters: Meters,
    },
    Time {
        seconds: f64,
    },
    /// Ends when the athlete presses lap.
    Open,
    Other {
        target_type: i64,
        value: Option<f64>,
    },
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawSchedule {
    id: FlexStr,
    name: FlexStr,
    start_day: Num,
    end_day: Num,
    entities: Vec<RawEntity>,
    programs: Vec<RawProgram>,
    week_stages: Vec<RawWeekStage>,
}

impl RawSchedule {
    fn into_schedule(self) -> Schedule {
        let programs: Vec<(Option<String>, PlannedWorkout)> = self
            .programs
            .iter()
            .map(|p| (p.id_in_plan.owned(), p.planned()))
            .collect();
        let mut entries: Vec<ScheduledWorkout> = self
            .entities
            .into_iter()
            .filter_map(|e| {
                let date = date_from_int(e.happen_day)?;
                let id_in_plan = e.id_in_plan.owned();
                let workout = programs
                    .iter()
                    .find(|(pid, _)| pid.is_some() && *pid == id_in_plan)
                    .map(|(_, w)| w.clone());
                Some(ScheduledWorkout {
                    date,
                    id_in_plan,
                    workout,
                })
            })
            .collect();
        entries.sort_by_key(|e| e.date);
        Schedule {
            plan_id: self.id.owned(),
            plan_name: self.name.owned(),
            plan_start: date_from_int(self.start_day),
            plan_end: date_from_int(self.end_day),
            entries,
            weeks: self
                .week_stages
                .into_iter()
                .filter_map(RawWeekStage::into_week)
                .collect(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawEntity {
    happen_day: Num,
    id_in_plan: FlexStr,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawWeekStage {
    first_day_in_week: Num,
    train_sum: RawTrainSum,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawTrainSum {
    plan_distance: Num,
    actual_distance: Num,
    plan_duration: Num,
    actual_duration: Num,
    plan_training_load: Num,
    actual_training_load: Num,
    plan_count: Num,
    actual_count: Num,
}

impl RawWeekStage {
    fn into_week(self) -> Option<WeekPlan> {
        let s = self.train_sum;
        Some(WeekPlan {
            week_start: date_from_int(self.first_day_in_week)?,
            plan_distance_raw: s.plan_distance.get(),
            actual_distance_raw: s.actual_distance.get(),
            plan_duration: duration(s.plan_duration.get()),
            actual_duration: duration(s.actual_duration.get()),
            plan_load: s.plan_training_load.get(),
            actual_load: s.actual_training_load.get(),
            plan_count: s.plan_count.get().map(|c| c.max(0.0).round() as u32),
            actual_count: s.actual_count.get().map(|c| c.max(0.0).round() as u32),
        })
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawProgram {
    id: FlexStr,
    id_in_plan: FlexStr,
    name: FlexStr,
    sport_type: Num,
    duration: Num,
    estimated_time: Num,
    distance: Num,
    training_load: Num,
    exercise_num: Num,
    exercises: Vec<RawExercise>,
}

impl RawProgram {
    fn planned(&self) -> PlannedWorkout {
        PlannedWorkout {
            id: self.id.owned(),
            name: self.name.owned(),
            sport_code: self
                .sport_type
                .get()
                .filter(|v| *v >= 0.0)
                .map(|v| v.round() as u32),
            duration: duration(self.duration.get().or(self.estimated_time.get())),
            distance_raw: self.distance.nonzero(),
            training_load: self.training_load.positive_u32(),
            exercise_count: self.exercise_num.positive_u32(),
        }
    }

    fn into_workout(self) -> Option<Workout> {
        Some(Workout {
            sport_code: self
                .sport_type
                .get()
                .filter(|v| *v >= 0.0)
                .map(|v| v.round() as u32),
            estimated_time: duration(self.estimated_time.get().or(self.duration.get())),
            training_load: self.training_load.positive_u32(),
            name: self.name.owned(),
            id: self.id.owned()?,
            steps: self
                .exercises
                .into_iter()
                .map(RawExercise::into_step)
                .collect(),
        })
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawExercise {
    name: FlexStr,
    sets: Num,
    target_type: Num,
    target_value: Num,
    intensity_value: Num,
    intensity_value_extend: Num,
    intensity_multiplier: Num,
}

impl RawExercise {
    fn into_step(self) -> WorkoutStep {
        let divisor = if self.intensity_multiplier.int() == Some(1000) {
            1000.0
        } else {
            1.0
        };
        let target = match self.target_type.int() {
            // Distance: metres ×100.
            Some(5) => match self.target_value.get() {
                Some(v) => StepTarget::Distance {
                    meters: Meters(v / 100.0),
                },
                None => StepTarget::Open,
            },
            // Open/manual step: 0 means "no cap", not "zero seconds".
            Some(1) => StepTarget::Open,
            Some(2) => StepTarget::Time {
                seconds: self.target_value.get().unwrap_or(0.0),
            },
            Some(other) => StepTarget::Other {
                target_type: other,
                value: self.target_value.get(),
            },
            None => StepTarget::Open,
        };
        WorkoutStep {
            name: self.name.owned(),
            sets: self.sets.positive_u32().unwrap_or(1),
            target,
            intensity_low: self.intensity_value.nonzero().map(|v| v / divisor),
            intensity_high: self.intensity_value_extend.nonzero().map(|v| v / divisor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_request_params() {
        let range = DateRange::new(
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
        )
        .unwrap();
        let r = schedule_request(range);
        assert_eq!(
            r.query,
            vec![
                ("startDate", "20261012".into()),
                ("endDate", "20261018".into()),
                ("supportRestExercise", "1".into())
            ]
        );
        assert!(workouts_request().idempotent);
    }
}
