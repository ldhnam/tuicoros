//! Activities: list, detail (summary, laps, samples, notes) and file export.
//!
//! Remember: the list is in plain units, the detail is centi-scaled (×100).

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::CorosClient;
use crate::error::{CorosError, Result};
use crate::request::ApiRequest;
use crate::sport::SportType;
use crate::units::{
    LabelId, Meters, Pace, Page, date_from_int, datetime, datetime_centi, decenti, duration,
    gps_degrees, kcal, meters, offset_from_quarters, ser_opt_secs, yyyymmdd,
};
use crate::util::{FlexStr, Num};

/// The API rejects pages larger than this.
pub const MAX_PAGE_SIZE: u32 = 200;

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Filter for `activity/query`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityQuery {
    /// 1-based.
    pub page: u32,
    /// 1..=200.
    pub size: u32,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// Empty = all sports.
    pub sports: Vec<SportType>,
}

impl Default for ActivityQuery {
    fn default() -> Self {
        Self {
            page: 1,
            size: 20,
            from: None,
            to: None,
            sports: Vec::new(),
        }
    }
}

impl ActivityQuery {
    fn validate(&self) -> Result<()> {
        if self.page < 1 {
            return Err(CorosError::InvalidInput("page must be at least 1".into()));
        }
        if !(1..=MAX_PAGE_SIZE).contains(&self.size) {
            return Err(CorosError::InvalidInput(format!(
                "page size must be 1..={MAX_PAGE_SIZE}"
            )));
        }
        if let (Some(from), Some(to)) = (self.from, self.to)
            && from > to
        {
            return Err(CorosError::InvalidInput(format!(
                "`from` ({from}) is after `to` ({to})"
            )));
        }
        Ok(())
    }
}

pub fn activities_request(query: &ActivityQuery) -> ApiRequest {
    let mut request = ApiRequest::get("activity/query")
        .query("pageNumber", query.page.to_string())
        .query("size", query.size.to_string());
    if let Some(from) = query.from {
        request = request.query("startDay", yyyymmdd(from));
    }
    if let Some(to) = query.to {
        request = request.query("endDay", yyyymmdd(to));
    }
    if !query.sports.is_empty() {
        let codes: Vec<String> = query.sports.iter().map(|s| s.code().to_string()).collect();
        request = request.query("modeList", codes.join(","));
    }
    request
}

pub fn activity_detail_request(id: &LabelId, sport: SportType) -> ApiRequest {
    ApiRequest::post("activity/detail/query", serde_json::json!({}))
        .read_only()
        .query("labelId", id.0.clone())
        .query("sportType", sport.code().to_string())
}

pub fn activity_download_request(id: &LabelId, sport: SportType, format: FileFormat) -> ApiRequest {
    ApiRequest::post("activity/detail/download", serde_json::json!({}))
        .read_only()
        .query("labelId", id.0.clone())
        .query("sportType", sport.code().to_string())
        .query("fileType", format.code().to_string())
}

/// Export formats for `activity/detail/download`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    Csv,
    Gpx,
    Kml,
    Tcx,
    Fit,
}

impl FileFormat {
    pub fn code(self) -> u8 {
        match self {
            FileFormat::Csv => 0,
            FileFormat::Gpx => 1,
            FileFormat::Kml => 2,
            FileFormat::Tcx => 3,
            FileFormat::Fit => 4,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            FileFormat::Csv => "csv",
            FileFormat::Gpx => "gpx",
            FileFormat::Kml => "kml",
            FileFormat::Tcx => "tcx",
            FileFormat::Fit => "fit",
        }
    }
}

impl std::str::FromStr for FileFormat {
    type Err = CorosError;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(FileFormat::Csv),
            "gpx" => Ok(FileFormat::Gpx),
            "kml" => Ok(FileFormat::Kml),
            "tcx" => Ok(FileFormat::Tcx),
            "fit" => Ok(FileFormat::Fit),
            other => Err(CorosError::InvalidInput(format!(
                "unknown file format `{other}` (fit, gpx, tcx, kml, csv)"
            ))),
        }
    }
}

impl CorosClient {
    /// `GET activity/query`: one page.
    pub async fn activities(&self, query: &ActivityQuery) -> Result<Page<ActivitySummary>> {
        query.validate()?;
        let raw: RawActivityPage = self.send(&activities_request(query)).await?;
        Ok(raw.into_page(query.page))
    }

    /// Every activity matching `query`, following pages from `query.page`.
    /// Stops at an empty page, at `total_pages`, or after `limit` items.
    pub async fn all_activities(
        &self,
        query: &ActivityQuery,
        limit: Option<usize>,
    ) -> Result<Vec<ActivitySummary>> {
        let mut query = query.clone();
        let mut out = Vec::new();
        loop {
            let page = self.activities(&query).await?;
            let done = page.items.is_empty() || query.page >= page.total_pages;
            out.extend(page.items);
            if let Some(limit) = limit
                && out.len() >= limit
            {
                out.truncate(limit);
                break;
            }
            if done {
                break;
            }
            query.page += 1;
        }
        Ok(out)
    }

    /// `POST activity/detail/query`. `sport` must be the activity's own sport
    /// (from the list); the API returns wrong or empty data otherwise.
    pub async fn activity_detail(&self, id: &LabelId, sport: SportType) -> Result<ActivityDetail> {
        let raw: RawDetail = self.send(&activity_detail_request(id, sport)).await?;
        Ok(raw.into_detail(id.clone()))
    }

    /// `POST activity/detail/download`: a short-lived signed URL for the file.
    pub async fn activity_file_url(
        &self,
        id: &LabelId,
        sport: SportType,
        format: FileFormat,
    ) -> Result<String> {
        let request = activity_download_request(id, sport, format);
        let raw: RawDownload = self.send(&request).await?;
        raw.file_url.owned().ok_or_else(|| CorosError::Unexpected {
            endpoint: request.path.to_owned(),
            detail: "no fileUrl in response".to_owned(),
            body_snippet: String::new(),
        })
    }

    /// Download an activity file's bytes.
    pub async fn download_activity_file(
        &self,
        id: &LabelId,
        sport: SportType,
        format: FileFormat,
    ) -> Result<Vec<u8>> {
        let url = self.activity_file_url(id, sport, format).await?;
        self.download(&url).await
    }
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/// One row of the activity list. Zero-filled API fields become `None`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivitySummary {
    pub id: LabelId,
    pub name: Option<String>,
    pub sport: SportType,
    /// Local calendar date of the activity.
    pub date: Option<NaiveDate>,
    /// Start time in the activity's own UTC offset.
    pub start: Option<DateTime<FixedOffset>>,
    pub end: Option<DateTime<FixedOffset>>,
    pub distance: Option<Meters>,
    /// Wall-clock time including pauses.
    #[serde(serialize_with = "ser_opt_secs")]
    pub elapsed: Option<Duration>,
    /// Moving time.
    #[serde(serialize_with = "ser_opt_secs")]
    pub moving: Option<Duration>,
    /// Computed from distance and moving (or elapsed) time, not from the API's
    /// `avgSpeed`, whose meaning depends on the sport.
    pub avg_pace: Option<Pace>,
    pub avg_hr: Option<u32>,
    pub max_hr: Option<u32>,
    pub avg_cadence: Option<u32>,
    pub avg_power: Option<u32>,
    pub normalized_power: Option<u32>,
    pub ascent: Option<Meters>,
    pub descent: Option<Meters>,
    pub calories_kcal: Option<f64>,
    pub training_load: Option<u32>,
    pub steps: Option<u32>,
    pub device: Option<String>,
}

/// Everything from `activity/detail/query` that we understand.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityDetail {
    pub id: LabelId,
    pub summary: DetailSummary,
    /// Lap groupings (auto-lap by distance, manual laps, …).
    pub lap_groups: Vec<LapGroup>,
    /// The recorded track, about one sample per second.
    pub samples: Vec<Sample>,
    pub feel: Feel,
    /// Weather block as sent by the API (shape not modelled yet).
    pub weather: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DetailSummary {
    pub name: Option<String>,
    pub sport: Option<SportType>,
    pub start: Option<DateTime<FixedOffset>>,
    pub end: Option<DateTime<FixedOffset>>,
    pub distance: Option<Meters>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub elapsed: Option<Duration>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub moving: Option<Duration>,
    pub avg_pace: Option<Pace>,
    pub avg_hr: Option<u32>,
    pub max_hr: Option<u32>,
    pub avg_cadence: Option<u32>,
    pub max_cadence: Option<u32>,
    pub avg_power: Option<u32>,
    pub max_power: Option<u32>,
    pub calories_kcal: Option<f64>,
    pub training_load: Option<u32>,
    /// Scale not yet verified against a live account (docs/COROS_API.md).
    pub elevation_gain: Option<f64>,
    /// Scale not yet verified against a live account.
    pub aerobic_effect: Option<f64>,
    /// Scale not yet verified against a live account.
    pub anaerobic_effect: Option<f64>,
    pub vo2max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LapGroup {
    /// The API's grouping kind (e.g. auto-lap vs manual). Meaning of each value
    /// to be confirmed live.
    pub kind: Option<i64>,
    pub laps: Vec<Lap>,
    /// 0-based indexes of the fastest laps, as flagged by COROS.
    pub fastest: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lap {
    pub index: u32,
    pub distance: Option<Meters>,
    #[serde(serialize_with = "ser_opt_secs")]
    pub duration: Option<Duration>,
    pub pace: Option<Pace>,
    pub avg_hr: Option<u32>,
    pub max_hr: Option<u32>,
    pub avg_cadence: Option<u32>,
    pub avg_power: Option<u32>,
    /// Scale not yet verified.
    pub elevation_gain: Option<f64>,
}

/// One sample of the recorded track.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    pub time: Option<DateTime<Utc>>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Raw numeric fields we don't model yet (heart rate, speed, …), keyed by
    /// API name, so they can be discovered with `--json`.
    pub other: BTreeMap<String, f64>,
}

/// The athlete's own rating and note.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Feel {
    pub perceived_exertion: Option<PerceivedExertion>,
    pub note: Option<String>,
}

/// COROS's five-point RPE scale (`feelType` 1..=5; 0 = not rated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PerceivedExertion {
    VeryLight,
    Light,
    Moderate,
    Hard,
    MaxEffort,
}

impl PerceivedExertion {
    fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            1 => Self::VeryLight,
            2 => Self::Light,
            3 => Self::Moderate,
            4 => Self::Hard,
            5 => Self::MaxEffort,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::VeryLight => "Very Light",
            Self::Light => "Light",
            Self::Moderate => "Moderate",
            Self::Hard => "Hard",
            Self::MaxEffort => "Max Effort",
        }
    }
}

// ---------------------------------------------------------------------------
// Raw JSON
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawActivityPage {
    count: Num,
    total_page: Num,
    page_number: Num,
    data_list: Vec<RawActivity>,
}

impl RawActivityPage {
    fn into_page(self, requested_page: u32) -> Page<ActivitySummary> {
        let items: Vec<ActivitySummary> = self
            .data_list
            .into_iter()
            .filter_map(RawActivity::into_summary)
            .collect();
        Page {
            page: self.page_number.positive_u32().unwrap_or(requested_page),
            total_pages: self.total_page.positive_u32().unwrap_or(0),
            total_count: self.count.positive_u32().unwrap_or(0),
            items,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawActivity {
    label_id: FlexStr,
    name: FlexStr,
    sport_type: Num,
    date: Num,
    start_time: Num,
    end_time: Num,
    start_timezone: Num,
    end_timezone: Num,
    distance: Num,
    total_time: Num,
    workout_time: Num,
    avg_hr: Num,
    max_hr: Num,
    avg_cadence: Num,
    avg_power: Num,
    np: Num,
    ascent: Num,
    descent: Num,
    calorie: Num,
    training_load: Num,
    step: Num,
    device: FlexStr,
}

impl RawActivity {
    /// Rows without an id are useless (can't open them); skip them.
    fn into_summary(self) -> Option<ActivitySummary> {
        let id = LabelId(self.label_id.owned()?);
        let start_tz = offset_from_quarters(self.start_timezone);
        let end_tz = offset_from_quarters(self.end_timezone).or(start_tz);
        let distance = meters(self.distance.get());
        let elapsed = duration(self.total_time.get());
        let moving = duration(self.workout_time.get());
        Some(ActivitySummary {
            id,
            name: self.name.owned(),
            sport: sport(self.sport_type),
            date: date_from_int(self.date),
            start: datetime(self.start_time.get(), start_tz),
            end: datetime(self.end_time.get(), end_tz),
            avg_pace: distance.and_then(|d| Pace::from_distance_time(d, moving.or(elapsed)?)),
            distance,
            elapsed,
            moving,
            avg_hr: self.avg_hr.positive_u32(),
            max_hr: self.max_hr.positive_u32(),
            avg_cadence: self.avg_cadence.positive_u32(),
            avg_power: self.avg_power.positive_u32(),
            normalized_power: self.np.positive_u32(),
            ascent: meters(self.ascent.get()),
            descent: meters(self.descent.get()),
            calories_kcal: kcal(self.calorie),
            training_load: self.training_load.positive_u32(),
            steps: self.step.positive_u32(),
            device: self.device.owned(),
        })
    }
}

fn sport(code: Num) -> SportType {
    SportType::from_code(code.positive_u32().unwrap_or(0))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawDetail {
    summary: RawDetailSummary,
    lap_list: Vec<RawLapGroup>,
    frequency_list: Vec<BTreeMap<String, Value>>,
    sport_feel_info: RawFeel,
    weather: Option<Value>,
}

impl RawDetail {
    fn into_detail(self, id: LabelId) -> ActivityDetail {
        ActivityDetail {
            id,
            summary: self.summary.into_summary(),
            lap_groups: self
                .lap_list
                .into_iter()
                .map(RawLapGroup::into_group)
                .collect(),
            samples: self.frequency_list.into_iter().map(sample).collect(),
            feel: self.sport_feel_info.into_feel(),
            weather: self.weather.filter(|w| !w.is_null()),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawDetailSummary {
    name: FlexStr,
    sport_type: Num,
    distance: Num,
    total_time: Num,
    workout_time: Num,
    start_timestamp: Num,
    end_timestamp: Num,
    timezone: Num,
    calories: Num,
    avg_hr: Num,
    max_hr: Num,
    avg_cadence: Num,
    max_cadence: Num,
    avg_power: Num,
    max_power: Num,
    training_load: Num,
    elev_gain: Num,
    aerobic_effect: Num,
    anaerobic_effect: Num,
    current_vo2_max: Num,
}

impl RawDetailSummary {
    fn into_summary(self) -> DetailSummary {
        let tz = offset_from_quarters(self.timezone);
        let distance = meters(decenti(self.distance));
        let elapsed = duration(decenti(self.total_time));
        let moving = duration(decenti(self.workout_time));
        DetailSummary {
            name: self.name.owned(),
            sport: self.sport_type.positive_u32().map(SportType::from_code),
            start: datetime_centi(self.start_timestamp, tz),
            end: datetime_centi(self.end_timestamp, tz),
            avg_pace: distance.and_then(|d| Pace::from_distance_time(d, moving.or(elapsed)?)),
            distance,
            elapsed,
            moving,
            avg_hr: self.avg_hr.positive_u32(),
            max_hr: self.max_hr.positive_u32(),
            avg_cadence: self.avg_cadence.positive_u32(),
            max_cadence: self.max_cadence.positive_u32(),
            avg_power: self.avg_power.positive_u32(),
            max_power: self.max_power.positive_u32(),
            calories_kcal: kcal(self.calories),
            training_load: self.training_load.positive_u32(),
            elevation_gain: self.elev_gain.nonzero(),
            aerobic_effect: self.aerobic_effect.nonzero(),
            anaerobic_effect: self.anaerobic_effect.nonzero(),
            vo2max: self.current_vo2_max.nonzero(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawLapGroup {
    #[serde(rename = "type")]
    kind: Num,
    fast_lap_index_list: Vec<Num>,
    lap_item_list: Vec<RawLap>,
}

impl RawLapGroup {
    fn into_group(self) -> LapGroup {
        LapGroup {
            kind: self.kind.int(),
            fastest: self
                .fast_lap_index_list
                .into_iter()
                .filter_map(|n| n.int().and_then(|i| u32::try_from(i).ok()))
                .collect(),
            laps: self
                .lap_item_list
                .into_iter()
                .enumerate()
                .map(|(i, lap)| lap.into_lap(i))
                .collect(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawLap {
    lap_index: Num,
    distance: Num,
    time: Num,
    avg_hr: Num,
    max_hr: Num,
    avg_cadence: Num,
    avg_power: Num,
    elev_gain: Num,
}

impl RawLap {
    fn into_lap(self, position: usize) -> Lap {
        let distance = meters(decenti(self.distance));
        let duration = duration(decenti(self.time));
        Lap {
            index: self
                .lap_index
                .int()
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(position as u32 + 1),
            pace: distance
                .zip(duration)
                .and_then(|(d, t)| Pace::from_distance_time(d, t)),
            distance,
            duration,
            avg_hr: self.avg_hr.positive_u32(),
            max_hr: self.max_hr.positive_u32(),
            avg_cadence: self.avg_cadence.positive_u32(),
            avg_power: self.avg_power.positive_u32(),
            elevation_gain: self.elev_gain.nonzero(),
        }
    }
}

fn sample(mut raw: BTreeMap<String, Value>) -> Sample {
    let mut take = |key: &str| Num(raw.remove(key).and_then(|v| v.as_f64()));
    let time = datetime_centi(take("timestamp"), None).map(|t| t.with_timezone(&Utc));
    let lat = gps_degrees(take("gpsLat"));
    let lon = gps_degrees(take("gpsLon"));
    let other = raw
        .into_iter()
        .filter_map(|(k, v)| v.as_f64().map(|f| (k, f)))
        .collect();
    Sample {
        time,
        lat,
        lon,
        other,
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawFeel {
    feel_type: Num,
    sport_note: FlexStr,
}

impl RawFeel {
    fn into_feel(self) -> Feel {
        Feel {
            perceived_exertion: self.feel_type.int().and_then(PerceivedExertion::from_code),
            note: self.sport_note.owned(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawDownload {
    file_url: FlexStr,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_request_params() {
        let q = ActivityQuery {
            page: 2,
            size: 50,
            from: NaiveDate::from_ymd_opt(2026, 9, 1),
            to: NaiveDate::from_ymd_opt(2026, 9, 30),
            sports: vec![SportType::Run, SportType::TrailRun],
        };
        let r = activities_request(&q);
        assert_eq!(r.path, "activity/query");
        assert_eq!(
            r.query,
            vec![
                ("pageNumber", "2".to_string()),
                ("size", "50".to_string()),
                ("startDay", "20260901".to_string()),
                ("endDay", "20260930".to_string()),
                ("modeList", "100,102".to_string()),
            ]
        );
        assert!(r.idempotent);
    }

    #[test]
    fn query_validation() {
        assert!(
            ActivityQuery {
                size: 201,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ActivityQuery {
                page: 0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let backwards = ActivityQuery {
            from: NaiveDate::from_ymd_opt(2026, 2, 1),
            to: NaiveDate::from_ymd_opt(2026, 1, 1),
            ..Default::default()
        };
        assert!(backwards.validate().is_err());
        assert!(ActivityQuery::default().validate().is_ok());
    }

    #[test]
    fn detail_and_download_are_retryable_posts_with_query_params() {
        let id = LabelId("4711".into());
        let r = activity_detail_request(&id, SportType::TrailRun);
        assert_eq!(r.method, crate::request::HttpMethod::Post);
        assert!(r.idempotent);
        assert_eq!(
            r.query,
            vec![("labelId", "4711".into()), ("sportType", "102".into())]
        );
        assert_eq!(r.body, Some(serde_json::json!({})));
        let d = activity_download_request(&id, SportType::Run, FileFormat::Gpx);
        assert!(d.query.contains(&("fileType", "1".into())));
    }

    #[test]
    fn file_formats() {
        assert_eq!("FIT".parse::<FileFormat>().unwrap(), FileFormat::Fit);
        assert_eq!(FileFormat::Tcx.code(), 3);
        assert!("doc".parse::<FileFormat>().is_err());
    }
}
