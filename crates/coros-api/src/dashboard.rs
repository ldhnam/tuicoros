//! `dashboard/query`: currently only the overnight HRV block is modelled.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::client::CorosClient;
use crate::error::Result;
use crate::request::ApiRequest;
use crate::units::date_from_int;
use crate::util::Num;

pub fn dashboard_request() -> ApiRequest {
    ApiRequest::get("dashboard/query")
}

impl CorosClient {
    /// `GET dashboard/query`. No date parameters: always "recent".
    pub async fn dashboard(&self) -> Result<Dashboard> {
        let raw: RawDashboard = self.send(&dashboard_request()).await?;
        Ok(raw.into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dashboard {
    pub hrv: Option<HrvSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HrvSummary {
    /// The most recent night (the block's own top-level values).
    pub latest: Option<HrvNight>,
    /// About the last 7 nights, oldest first.
    pub nights: Vec<HrvNight>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HrvNight {
    pub date: NaiveDate,
    /// Average overnight HRV, ms.
    pub avg: Option<f64>,
    /// Personal baseline, ms.
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
    sleep_hrv_data: Option<RawHrvData>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawHrvData {
    #[serde(flatten)]
    latest: RawHrvNight,
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
    fn into_night(self) -> Option<HrvNight> {
        Some(HrvNight {
            date: date_from_int(self.happen_day)?,
            avg: self.avg_sleep_hrv.nonzero(),
            baseline: self.sleep_hrv_base.nonzero(),
            std_dev: self.sleep_hrv_sd.nonzero(),
        })
    }
}

impl From<RawDashboard> for Dashboard {
    fn from(raw: RawDashboard) -> Self {
        let hrv = raw.summary_info.sleep_hrv_data.map(|data| {
            let mut nights: Vec<HrvNight> = data
                .sleep_hrv_list
                .into_iter()
                .filter_map(RawHrvNight::into_night)
                .collect();
            nights.sort_by_key(|n| n.date);
            HrvSummary {
                latest: data.latest.into_night(),
                nights,
            }
        });
        Dashboard { hrv }
    }
}
