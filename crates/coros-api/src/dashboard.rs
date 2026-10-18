//! `dashboard/query`: recovery, thresholds, ability scores and overnight HRV.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::client::CorosClient;
use crate::error::Result;
use crate::request::ApiRequest;
use crate::units::{Pace, date_from_int};
use crate::util::Num;

pub fn dashboard_request() -> ApiRequest {
    ApiRequest::get("dashboard/query")
}

impl CorosClient {
    /// `GET dashboard/query`. No date parameters: always "now".
    pub async fn dashboard(&self) -> Result<Dashboard> {
        let raw: RawDashboard = self.send(&dashboard_request()).await?;
        Ok(raw.into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dashboard {
    /// Recovery, 0–100 %.
    pub recovery_pct: Option<f64>,
    /// Hours until fully recovered.
    pub full_recovery_hours: Option<f64>,
    pub resting_hr: Option<u32>,
    pub max_hr: Option<u32>,
    /// Lactate threshold heart rate.
    pub lthr: Option<u32>,
    /// Lactate threshold pace.
    pub lt_pace: Option<Pace>,
    pub stamina_level: Option<f64>,
    pub scores: AbilityScores,
    pub hrv: Option<HrvSummary>,
}

/// EvoLab running ability scores (0–100).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AbilityScores {
    pub aerobic_endurance: Option<f64>,
    pub lactate_threshold: Option<f64>,
    pub anaerobic_endurance: Option<f64>,
    pub anaerobic_capacity: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HrvSummary {
    /// The most recent night, if COROS has a value for it.
    pub latest: Option<HrvNight>,
    /// Recent nights with a value, oldest first.
    pub nights: Vec<HrvNight>,
    /// Personal baseline, ms.
    pub baseline: Option<f64>,
    /// Nights of wear still needed before COROS shows HRV.
    pub remaining_wear_days: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HrvNight {
    pub date: NaiveDate,
    /// Average overnight HRV, ms.
    pub avg: f64,
    pub baseline: Option<f64>,
    pub std_dev: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawDashboard {
    summary_info: RawSummaryInfo,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawSummaryInfo {
    recovery_pct: Num,
    full_recovery_hours: Num,
    rhr: Num,
    fitness_max_hr: Num,
    lthr: Num,
    ltsp: Num,
    stamina_level: Num,
    aerobic_endurance_score: Num,
    lactate_threshold_capacity_score: Num,
    anaerobic_endurance_score: Num,
    anaerobic_capacity_score: Num,
    sleep_hrv_data: Option<RawHrvData>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawHrvData {
    #[serde(flatten)]
    latest: RawHrvNight,
    last_sleep_hrv_base: Num,
    remain_wear_days: Num,
    sleep_hrv_list: Vec<RawHrvNight>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawHrvNight {
    happen_day: Num,
    avg_sleep_hrv: Num,
    sleep_hrv_base: Num,
    sleep_hrv_sd: Num,
}

impl RawHrvNight {
    /// Nights without an average (not enough data yet) are skipped.
    fn into_night(self) -> Option<HrvNight> {
        Some(HrvNight {
            date: date_from_int(self.happen_day)?,
            avg: self.avg_sleep_hrv.nonzero()?,
            baseline: self.sleep_hrv_base.nonzero(),
            std_dev: self.sleep_hrv_sd.nonzero(),
        })
    }
}

impl From<RawDashboard> for Dashboard {
    fn from(raw: RawDashboard) -> Self {
        let info = raw.summary_info;
        let hrv = info.sleep_hrv_data.map(|data| {
            let baseline = data
                .latest
                .sleep_hrv_base
                .nonzero()
                .or(data.last_sleep_hrv_base.nonzero());
            let mut nights: Vec<HrvNight> = data
                .sleep_hrv_list
                .into_iter()
                .filter_map(RawHrvNight::into_night)
                .collect();
            nights.sort_by_key(|n| n.date);
            HrvSummary {
                latest: data.latest.into_night(),
                nights,
                baseline,
                remaining_wear_days: data.remain_wear_days.positive_u32(),
            }
        });
        Dashboard {
            recovery_pct: info.recovery_pct.get(),
            full_recovery_hours: info.full_recovery_hours.get(),
            resting_hr: info.rhr.positive_u32(),
            max_hr: info.fitness_max_hr.positive_u32(),
            lthr: info.lthr.positive_u32(),
            lt_pace: info.ltsp.get().filter(|p| *p > 0.0).map(Pace),
            stamina_level: info.stamina_level.nonzero(),
            scores: AbilityScores {
                aerobic_endurance: info.aerobic_endurance_score.nonzero(),
                lactate_threshold: info.lactate_threshold_capacity_score.nonzero(),
                anaerobic_endurance: info.anaerobic_endurance_score.nonzero(),
                anaerobic_capacity: info.anaerobic_capacity_score.nonzero(),
            },
            hrv,
        }
    }
}
