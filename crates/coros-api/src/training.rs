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
use crate::units::{
    DateRange, LabelId, Meters, Pace, date_from_int, decenti, duration, meters, yyyymmdd,
};
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

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub plan_id: Option<String>,
    pub plan_name: Option<String>,
    pub plan_start: Option<NaiveDate>,
    pub plan_end: Option<NaiveDate>,
    /// Calendar entries, sorted by date.
    pub entries: Vec<ScheduledWorkout>,
    pub weeks: Vec<WeekPlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledWorkout {
    pub date: NaiveDate,
    pub id_in_plan: Option<String>,
    /// The workout definition, when the entry links to one in `programs`.
    pub workout: Option<PlannedWorkout>,
    /// The activity that completed this entry, if any.
    pub activity_id: Option<LabelId>,
    /// How much of the workout was done, 0–1 (`completeRate`).
    pub completion: Option<f64>,
    /// COROS's execution score, 0–100.
    pub score: Option<f64>,
}

/// A workout as it appears on the calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedWorkout {
    pub id: Option<String>,
    pub name: Option<String>,
    pub sport_code: Option<u32>,
    #[serde(with = "crate::units::opt_secs", default)]
    pub duration: Option<Duration>,
    pub distance: Option<Meters>,
    pub training_load: Option<u32>,
    pub exercise_count: Option<u32>,
}

/// Plan vs actual totals for one week.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeekPlan {
    pub week_start: NaiveDate,
    pub plan_distance: Option<Meters>,
    pub actual_distance: Option<Meters>,
    #[serde(with = "crate::units::opt_secs", default)]
    pub plan_duration: Option<Duration>,
    #[serde(with = "crate::units::opt_secs", default)]
    pub actual_duration: Option<Duration>,
    pub plan_load: Option<f64>,
    pub actual_load: Option<f64>,
}

/// A workout from the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workout {
    pub id: String,
    pub name: Option<String>,
    /// Workout-namespace sport code (1 = run, 2 = indoor bike, 4 = strength, 200 = road bike).
    pub sport_code: Option<u32>,
    #[serde(with = "crate::units::opt_secs", default)]
    pub estimated_time: Option<Duration>,
    pub distance: Option<Meters>,
    pub training_load: Option<u32>,
    /// Top-level steps; repeat groups hold their own steps.
    pub steps: Vec<WorkoutStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkoutStep {
    /// Readable name: the custom name, or one derived from the step kind.
    pub label: String,
    pub kind: Option<StepKind>,
    /// Repeat count for groups; usually 1 otherwise.
    pub sets: u32,
    pub target: StepTarget,
    pub intensity: Option<Intensity>,
    /// Children of a repeat group.
    pub steps: Vec<WorkoutStep>,
}

/// What a workout step (or a workout-step lap) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// A repeat block (`exerciseType` 0).
    Group,
    /// `1` (not yet seen live; assumed from the Training Hub's step types).
    WarmUp,
    /// `2`: the work part (verified).
    Training,
    /// `3` (not yet seen live; assumed).
    CoolDown,
    /// `4`: recovery between reps (verified).
    Recovery,
    Other(i64),
}

impl StepKind {
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => Self::Group,
            1 => Self::WarmUp,
            2 => Self::Training,
            3 => Self::CoolDown,
            4 => Self::Recovery,
            other => Self::Other(other),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Group => "Repeat",
            Self::WarmUp => "Warm up",
            Self::Training => "Training",
            Self::CoolDown => "Cool down",
            Self::Recovery => "Recovery",
            Self::Other(_) => "Step",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Intensity {
    /// `intensityType` 3: a pace range.
    Pace { fastest: Pace, slowest: Pace },
    /// Any other intensity type, values as sent (after the ×1000 multiplier).
    Other {
        intensity_type: i64,
        low: Option<f64>,
        high: Option<f64>,
    },
}

// ---------------------------------------------------------------------------
// Raw JSON
// ---------------------------------------------------------------------------

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
                    activity_id: e.label_id.owned().filter(|id| id != "0").map(LabelId),
                    completion: e.complete_rate.get(),
                    score: e.score.get(),
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
    label_id: FlexStr,
    complete_rate: Num,
    score: Num,
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
}

impl RawWeekStage {
    fn into_week(self) -> Option<WeekPlan> {
        let s = self.train_sum;
        Some(WeekPlan {
            week_start: date_from_int(self.first_day_in_week)?,
            // Strings of metres ×100 ("3265015.00" = 32.65 km).
            plan_distance: meters(decenti(s.plan_distance)),
            actual_distance: meters(decenti(s.actual_distance)),
            plan_duration: duration(s.plan_duration.get()),
            actual_duration: duration(s.actual_duration.get()),
            plan_load: s.plan_training_load.get(),
            actual_load: s.actual_training_load.get(),
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
    estimated_distance: Num,
    training_load: Num,
    exercise_num: Num,
    exercises: Vec<RawExercise>,
}

impl RawProgram {
    fn sport_code(&self) -> Option<u32> {
        self.sport_type
            .get()
            .filter(|v| *v >= 0.0)
            .map(|v| v.round() as u32)
    }

    /// Program distances are metres ×100 (560000 = 8 × 700 m).
    fn distance(&self) -> Option<Meters> {
        meters(decenti(self.distance)).or(meters(decenti(self.estimated_distance)))
    }

    fn planned(&self) -> PlannedWorkout {
        PlannedWorkout {
            id: self.id.owned(),
            name: self.name.owned(),
            sport_code: self.sport_code(),
            duration: duration(self.duration.get().or(self.estimated_time.get())),
            distance: self.distance(),
            training_load: self.training_load.positive_u32(),
            exercise_count: self.exercise_num.positive_u32(),
        }
    }

    fn into_workout(self) -> Option<Workout> {
        let is_run = self.sport_code() == Some(1);
        Some(Workout {
            sport_code: self.sport_code(),
            estimated_time: duration(self.estimated_time.get().or(self.duration.get())),
            distance: self.distance(),
            training_load: self.training_load.positive_u32(),
            name: self.name.owned(),
            id: self.id.owned()?,
            steps: build_steps(self.exercises, is_run),
        })
    }
}

#[derive(Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
struct RawExercise {
    id: FlexStr,
    name: FlexStr,
    exercise_type: Num,
    is_group: Option<bool>,
    group_id: FlexStr,
    sort_no: Num,
    sets: Num,
    target_type: Num,
    target_value: Num,
    intensity_type: Num,
    intensity_value: Num,
    intensity_value_extend: Num,
    intensity_multiplier: Num,
}

/// Steps come as a flat list; children point to their group via `groupId`.
fn build_steps(mut exercises: Vec<RawExercise>, is_run: bool) -> Vec<WorkoutStep> {
    exercises.sort_by(|a, b| {
        a.sort_no
            .get()
            .unwrap_or(0.0)
            .partial_cmp(&b.sort_no.get().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let is_top = |e: &RawExercise| matches!(e.group_id.get(), None | Some("0"));
    let (top, children): (Vec<_>, Vec<_>) = exercises.into_iter().partition(is_top);
    top.into_iter()
        .map(|parent| {
            let id = parent.id.owned();
            let mut step = parent.into_step(is_run);
            if let Some(id) = id {
                step.steps = children
                    .iter()
                    .filter(|c| c.group_id.get() == Some(id.as_str()))
                    .map(|c| c.clone().into_step(is_run))
                    .collect();
            }
            step
        })
        .collect()
}

impl RawExercise {
    fn into_step(self, is_run: bool) -> WorkoutStep {
        let kind = if self.is_group == Some(true) {
            Some(StepKind::Group)
        } else {
            self.exercise_type.int().map(StepKind::from_code)
        };
        let multiplier = if self.intensity_multiplier.int() == Some(1000) {
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
            Some(2) => StepTarget::Time {
                seconds: self.target_value.get().unwrap_or(0.0),
            },
            // Open/manual step: 0 means "no cap", not "zero seconds". 0 = group header.
            Some(0 | 1) | None => StepTarget::Open,
            Some(other) => StepTarget::Other {
                target_type: other,
                value: self.target_value.get(),
            },
        };
        let low = self.intensity_value.nonzero().map(|v| v / multiplier);
        let high = self
            .intensity_value_extend
            .nonzero()
            .map(|v| v / multiplier);
        let intensity = match (self.intensity_type.int(), low, high) {
            (Some(0) | None, None, None) => None,
            (Some(3), Some(a), Some(b)) => Some(Intensity::Pace {
                fastest: Pace(a.min(b)),
                slowest: Pace(a.max(b)),
            }),
            (Some(t), low, high) => Some(Intensity::Other {
                intensity_type: t,
                low,
                high,
            }),
            (None, low, high) => Some(Intensity::Other {
                intensity_type: 0,
                low,
                high,
            }),
        };
        WorkoutStep {
            label: step_label(self.name.get(), kind, is_run),
            kind,
            sets: self.sets.positive_u32().unwrap_or(1),
            target,
            intensity,
            steps: Vec::new(),
        }
    }
}

/// Built-in steps are named with codes like `T3001`; show the step kind instead.
fn step_label(name: Option<&str>, kind: Option<StepKind>, is_run: bool) -> String {
    let is_code =
        |n: &str| n.len() > 1 && n.starts_with('T') && n[1..].chars().all(|c| c.is_ascii_digit());
    match (name, kind) {
        (_, Some(StepKind::Group)) => "Repeat".to_owned(),
        (Some(n), _) if !is_code(n) && !n.is_empty() => n.to_owned(),
        (_, Some(StepKind::Training)) if is_run => "Run".to_owned(),
        (_, Some(k)) => k.label().to_owned(),
        (Some(n), None) => n.to_owned(),
        (None, None) => "Step".to_owned(),
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

    #[test]
    fn step_labels() {
        assert_eq!(
            step_label(Some("T3001"), Some(StepKind::Training), true),
            "Run"
        );
        assert_eq!(
            step_label(Some("T3001"), Some(StepKind::Training), false),
            "Training"
        );
        assert_eq!(
            step_label(Some("T1123"), Some(StepKind::Recovery), true),
            "Recovery"
        );
        assert_eq!(
            step_label(Some("Strides"), Some(StepKind::Training), true),
            "Strides"
        );
        assert_eq!(
            step_label(Some("Training"), Some(StepKind::Group), true),
            "Repeat"
        );
    }
}
