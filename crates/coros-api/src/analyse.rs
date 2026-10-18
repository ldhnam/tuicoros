//! EvoLab analytics: `analyse/query` and `analyse/dayDetail/query`.
//! Values here are NOT centi-scaled.

use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::client::CorosClient;
use crate::error::Result;
use crate::request::ApiRequest;
use crate::units::{DateRange, Meters, Pace, date_from_int, duration, meters, yyyymmdd};
use crate::util::Num;

/// `analyse/dayDetail/query` is fetched in chunks of at most this many days.
pub const DAY_DETAIL_CHUNK_DAYS: u32 = 84;

pub fn analyse_request(range: Option<DateRange>) -> ApiRequest {
    let request = ApiRequest::get("analyse/query");
    match range {
        Some(r) => request
            .query("startDay", yyyymmdd(r.start))
            .query("endDay", yyyymmdd(r.end)),
        None => request,
    }
}

pub fn day_detail_request(range: DateRange) -> ApiRequest {
    ApiRequest::get("analyse/dayDetail/query")
        .query("startDay", yyyymmdd(range.start))
        .query("endDay", yyyymmdd(range.end))
}

impl CorosClient {
    /// `GET analyse/query` (GET only: POST answers `1001`). Without a range the
    /// server returns roughly the last 28 days.
    pub async fn analyse(&self, range: Option<DateRange>) -> Result<Analyse> {
        let raw: RawAnalyse = self.send(&analyse_request(range)).await?;
        Ok(raw.into())
    }

    /// `GET analyse/dayDetail/query`: daily metrics over long ranges, fetched in
    /// 12-week chunks and returned sorted by date.
    pub async fn day_detail(&self, range: DateRange) -> Result<Vec<DailyMetrics>> {
        let mut days = Vec::new();
        for chunk in range.chunks(DAY_DETAIL_CHUNK_DAYS) {
            let raw: RawDayList = self.send(&day_detail_request(chunk)).await?;
            days.extend(raw.day_list.into_iter().filter_map(RawDay::into_metrics));
        }
        days.sort_by_key(|d| d.date);
        days.dedup_by_key(|d| d.date);
        Ok(days)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Analyse {
    /// One entry per day in the range.
    pub days: Vec<DailyMetrics>,
    /// Rolling 7-day view (carries VO2max / threshold fields).
    pub rolling_7d: Vec<DailyMetrics>,
    pub weeks: Vec<WeekLoad>,
    /// `false` until EvoLab has enough data to be meaningful.
    pub model_valid: Option<bool>,
}

/// One day of EvoLab metrics. Days without watch data have most fields `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyMetrics {
    pub date: NaiveDate,
    pub distance: Option<Meters>,
    #[serde(with = "crate::units::opt_secs", default)]
    pub duration: Option<Duration>,
    /// Daily aggregate resting HR (what the web dashboard shows).
    pub rhr: Option<u32>,
    /// Measured resting HR (what the phone app shows). Prefer this one.
    pub test_rhr: Option<u32>,
    pub training_load: Option<u32>,
    /// `t7d`: total training load of the last 7 days (verified: equals the
    /// week's actual load in the training calendar).
    pub load_7d: Option<f64>,
    /// `t28d`: total training load of the last 28 days.
    pub load_28d: Option<f64>,
    /// `ati`: acute (short-term) training load.
    pub acute_load: Option<f64>,
    /// `cti`: chronic (long-term) training load, i.e. fitness.
    pub chronic_load: Option<f64>,
    pub load_ratio: Option<f64>,
    pub load_ratio_state: Option<i64>,
    /// `tiredRateNew`: fatigue = acute − chronic load (can be negative = fresh).
    pub fatigue: Option<f64>,
    pub fatigue_state: Option<i64>,
    /// Running performance trend as sent by the API (scale unclear).
    /// `-1` ("no estimate") becomes `None`.
    pub performance: Option<f64>,
    pub recommended_load_min: Option<f64>,
    pub recommended_load_max: Option<f64>,
    /// Overnight HRV, ms.
    pub avg_sleep_hrv: Option<f64>,
    pub sleep_hrv_baseline: Option<f64>,
    pub vo2max: Option<f64>,
    /// Lactate threshold heart rate, bpm.
    pub lthr: Option<u32>,
    /// Lactate threshold pace (`ltsp`, s/km).
    pub lt_pace: Option<Pace>,
    pub stamina_level: Option<f64>,
    pub stamina_level_7d: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeekLoad {
    pub week_start: NaiveDate,
    pub training_load: Option<f64>,
    pub recommended_min: Option<f64>,
    pub recommended_max: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawAnalyse {
    day_list: Vec<RawDay>,
    t7day_list: Vec<RawDay>,
    week_list: Vec<RawWeek>,
    sport_data_summary: Option<RawSportDataSummary>,
}

impl From<RawAnalyse> for Analyse {
    fn from(raw: RawAnalyse) -> Self {
        Analyse {
            days: raw
                .day_list
                .into_iter()
                .filter_map(RawDay::into_metrics)
                .collect(),
            rolling_7d: raw
                .t7day_list
                .into_iter()
                .filter_map(RawDay::into_metrics)
                .collect(),
            weeks: raw
                .week_list
                .into_iter()
                .filter_map(RawWeek::into_week)
                .collect(),
            model_valid: raw.sport_data_summary.and_then(|s| s.model_valid_state),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawSportDataSummary {
    model_valid_state: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawDayList {
    day_list: Vec<RawDay>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawDay {
    happen_day: Num,
    distance: Num,
    duration: Num,
    rhr: Num,
    test_rhr: Num,
    training_load: Num,
    t7d: Num,
    t28d: Num,
    ati: Num,
    cti: Num,
    training_load_ratio: Num,
    training_load_ratio_state: Num,
    tired_rate_new: Num,
    tired_rate_state_new: Num,
    performance: Num,
    recomend_tl_min: Num,
    recomend_tl_max: Num,
    avg_sleep_hrv: Num,
    sleep_hrv_base: Num,
    vo2max: Num,
    lthr: Num,
    ltsp: Num,
    stamina_level: Num,
    stamina_level7d: Num,
}

impl RawDay {
    fn into_metrics(self) -> Option<DailyMetrics> {
        Some(DailyMetrics {
            date: date_from_int(self.happen_day)?,
            distance: meters(self.distance.get()),
            duration: duration(self.duration.get()),
            rhr: self.rhr.positive_u32(),
            test_rhr: self.test_rhr.positive_u32(),
            training_load: self.training_load.positive_u32(),
            load_7d: self.t7d.nonzero(),
            load_28d: self.t28d.nonzero(),
            acute_load: self.ati.nonzero(),
            chronic_load: self.cti.nonzero(),
            load_ratio: self.training_load_ratio.nonzero(),
            load_ratio_state: self.training_load_ratio_state.int(),
            fatigue: self.tired_rate_new.nonzero(),
            fatigue_state: self.tired_rate_state_new.int(),
            performance: self.performance.get().filter(|p| *p >= 0.0),
            recommended_load_min: self.recomend_tl_min.nonzero(),
            recommended_load_max: self.recomend_tl_max.nonzero(),
            avg_sleep_hrv: self.avg_sleep_hrv.nonzero(),
            sleep_hrv_baseline: self.sleep_hrv_base.nonzero(),
            vo2max: self.vo2max.nonzero(),
            lthr: self.lthr.positive_u32(),
            lt_pace: self.ltsp.get().filter(|p| *p > 0.0).map(Pace),
            stamina_level: self.stamina_level.nonzero(),
            stamina_level_7d: self.stamina_level7d.nonzero(),
        })
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawWeek {
    first_day_of_week: Num,
    training_load: Num,
    recomend_tl_min: Num,
    recomend_tl_max: Num,
}

impl RawWeek {
    fn into_week(self) -> Option<WeekLoad> {
        Some(WeekLoad {
            week_start: date_from_int(self.first_day_of_week)?,
            training_load: self.training_load.get(),
            recommended_min: self.recomend_tl_min.nonzero(),
            recommended_max: self.recomend_tl_max.nonzero(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_use_start_day_end_day_and_get() {
        let range = DateRange::new(
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
        )
        .unwrap();
        let r = analyse_request(Some(range));
        assert_eq!(r.method, crate::request::HttpMethod::Get);
        assert_eq!(
            r.query,
            vec![
                ("startDay", "20260701".into()),
                ("endDay", "20260731".into())
            ]
        );
        assert!(analyse_request(None).query.is_empty());
        assert_eq!(day_detail_request(range).path, "analyse/dayDetail/query");
    }
}
