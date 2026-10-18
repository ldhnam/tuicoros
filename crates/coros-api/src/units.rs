//! The one place where COROS's units are converted (see docs/COROS_API.md §4).
//!
//! - list endpoints: metres, seconds, epoch seconds
//! - detail endpoint: the same fields ×100 ("centi-scaled")
//! - time zones: quarter-hours east of UTC (`8` = UTC+2)
//! - dates: `YYYYMMDD` integers
//! - GPS: degrees ×1e7
//! - calories: calories (divide by 1000 for kcal)

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, NaiveDate, Offset, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CorosError, Result};
use crate::util::Num;

/// A distance in metres.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Meters(pub f64);

impl Meters {
    pub fn km(self) -> f64 {
        self.0 / 1000.0
    }
}

/// A pace in seconds per kilometre.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pace(pub f64);

impl Pace {
    /// Below this distance a pace is meaningless (GPS noise, stationary sessions).
    const MIN_DISTANCE_M: f64 = 50.0;

    pub fn from_distance_time(distance: Meters, time: Duration) -> Option<Pace> {
        let secs = time.as_secs_f64();
        (distance.0 >= Self::MIN_DISTANCE_M && secs > 0.0).then(|| Pace(secs / distance.km()))
    }

    pub fn secs_per_km(self) -> f64 {
        self.0
    }

    pub fn speed_kmh(self) -> f64 {
        if self.0 > 0.0 { 3600.0 / self.0 } else { 0.0 }
    }
}

/// An activity id (`labelId`). Kept as a string: it is a large number that
/// does not always fit JavaScript-safe integers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LabelId(pub String);

impl fmt::Display for LabelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad`, not `write_str`, so `{:<20}` alignment works.
        f.pad(&self.0)
    }
}

/// One page of a paginated list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// 1-based.
    pub page: u32,
    pub total_pages: u32,
    pub total_count: u32,
}

/// An inclusive date range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl DateRange {
    pub fn new(start: NaiveDate, end: NaiveDate) -> Result<Self> {
        if start > end {
            return Err(CorosError::InvalidInput(format!(
                "date range starts after it ends ({start} > {end})"
            )));
        }
        Ok(Self { start, end })
    }

    /// The `days` days ending on (and including) `end`.
    pub fn last_days(end: NaiveDate, days: u32) -> Self {
        let start = end - chrono::Duration::days(i64::from(days.max(1)) - 1);
        Self { start, end }
    }

    pub fn days(&self) -> i64 {
        (self.end - self.start).num_days() + 1
    }

    /// Split into consecutive chunks of at most `max_days` days.
    pub fn chunks(&self, max_days: u32) -> Vec<DateRange> {
        let step = i64::from(max_days.max(1));
        let mut out = Vec::new();
        let mut start = self.start;
        while start <= self.end {
            let end = (start + chrono::Duration::days(step - 1)).min(self.end);
            out.push(DateRange { start, end });
            start = end + chrono::Duration::days(1);
        }
        out
    }
}

/// `YYYYMMDD`, the date format every COROS endpoint takes.
pub fn yyyymmdd(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

/// `20261018` → 2026-10-18. Zero or nonsense → `None`.
pub(crate) fn date_from_int(v: Num) -> Option<NaiveDate> {
    let n = v.int().filter(|n| *n > 0)?;
    let (y, m, d) = (n / 10_000, (n / 100) % 100, n % 100);
    NaiveDate::from_ymd_opt(
        i32::try_from(y).ok()?,
        u32::try_from(m).ok()?,
        u32::try_from(d).ok()?,
    )
}

/// Quarter-hours east of UTC → offset. `8` → +02:00, `22` → +05:30.
pub(crate) fn offset_from_quarters(v: Num) -> Option<FixedOffset> {
    let q = v.int()?;
    if !(-14 * 4..=14 * 4).contains(&q) {
        return None;
    }
    FixedOffset::east_opt(i32::try_from(q * 15 * 60).ok()?)
}

/// Epoch seconds (possibly fractional) → date-time in `offset` (UTC if unknown).
pub(crate) fn datetime(
    epoch_secs: Option<f64>,
    offset: Option<FixedOffset>,
) -> Option<DateTime<FixedOffset>> {
    let secs = epoch_secs.filter(|s| s.is_finite() && *s > 0.0)?;
    let whole = secs.floor();
    let nanos = ((secs - whole) * 1e9).round().min(999_999_999.0) as u32;
    let utc: DateTime<Utc> = DateTime::from_timestamp(whole as i64, nanos)?;
    Some(utc.with_timezone(&offset.unwrap_or(Utc.fix())))
}

/// Centi-scaled epoch seconds (detail endpoint) → date-time. Integer maths, so
/// `178464143013` is exactly `1784641430.13`.
pub(crate) fn datetime_centi(v: Num, offset: Option<FixedOffset>) -> Option<DateTime<FixedOffset>> {
    let centi = v.int().filter(|c| *c > 0)?;
    let nanos = u32::try_from((centi % 100) * 10_000_000).ok()?;
    let utc: DateTime<Utc> = DateTime::from_timestamp(centi / 100, nanos)?;
    Some(utc.with_timezone(&offset.unwrap_or(Utc.fix())))
}

/// Remove the detail endpoint's ×100 scaling.
pub(crate) fn decenti(v: Num) -> Option<f64> {
    v.get().map(|x| x / 100.0)
}

pub(crate) fn duration(secs: Option<f64>) -> Option<Duration> {
    secs.filter(|s| *s > 0.0)
        .and_then(|s| Duration::try_from_secs_f64(s).ok())
}

pub(crate) fn meters(m: Option<f64>) -> Option<Meters> {
    m.filter(|m| m.is_finite() && *m > 0.0).map(Meters)
}

/// Degrees ×1e7 → degrees. Zero means "no fix".
pub(crate) fn gps_degrees(v: Num) -> Option<f64> {
    let deg = v.nonzero()? / 1e7;
    (-180.0..=180.0).contains(&deg).then_some(deg)
}

/// Calories → kilocalories.
pub(crate) fn kcal(v: Num) -> Option<f64> {
    v.get().filter(|c| *c > 0.0).map(|c| c / 1000.0)
}

/// `Option<Duration>` as fractional seconds, both ways (readable `--json`,
/// round-trips through the on-disk cache).
pub(crate) mod opt_secs {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_some(&d.as_secs_f64()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.and_then(|s| Duration::try_from_secs_f64(s).ok()))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Timelike};
    use proptest::prelude::*;

    use super::*;

    fn n(v: f64) -> Num {
        Num(Some(v))
    }

    #[test]
    fn dates() {
        assert_eq!(
            date_from_int(n(20261018.0)),
            NaiveDate::from_ymd_opt(2026, 10, 18)
        );
        assert_eq!(date_from_int(n(0.0)), None);
        assert_eq!(date_from_int(n(20261340.0)), None);
        assert_eq!(date_from_int(Num(None)), None);
        assert_eq!(
            yyyymmdd(NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()),
            "20260105"
        );
    }

    #[test]
    fn quarter_hour_time_zones() {
        assert_eq!(
            offset_from_quarters(n(8.0)).unwrap().local_minus_utc(),
            2 * 3600
        );
        assert_eq!(
            offset_from_quarters(n(-20.0)).unwrap().local_minus_utc(),
            -5 * 3600
        );
        assert_eq!(
            offset_from_quarters(n(22.0)).unwrap().local_minus_utc(),
            5 * 3600 + 1800
        );
        assert_eq!(
            offset_from_quarters(n(28.0)).unwrap().local_minus_utc(),
            7 * 3600
        ); // Vietnam
        assert_eq!(offset_from_quarters(n(99.0)), None);
    }

    #[test]
    fn list_and_detail_timestamps_agree() {
        // Same activity from both endpoints (values from the Pinta365/coros notes).
        let tz = offset_from_quarters(n(8.0));
        let from_list = datetime(Some(1_784_641_430.0), tz).unwrap();
        let from_detail = datetime_centi(n(178_464_143_013.0), tz).unwrap();
        assert_eq!(from_list.timestamp(), from_detail.timestamp());
        assert_eq!(from_detail.nanosecond(), 130_000_000);
        assert_eq!(from_list.offset().local_minus_utc(), 7200);
        assert_eq!(from_list.year(), 2026);
        assert_eq!(datetime(Some(0.0), tz), None);
        assert!(datetime(Some(1_784_641_430.0), None).is_some());
    }

    #[test]
    fn centi_scaling() {
        assert_eq!(decenti(n(235_759.0)), Some(2357.59));
        assert_eq!(decenti(n(691_549.0)), Some(6915.49));
    }

    #[test]
    fn gps_and_calories() {
        assert!((gps_degrees(n(577_528_390.0)).unwrap() - 57.752_839).abs() < 1e-9);
        assert_eq!(gps_degrees(n(0.0)), None);
        assert_eq!(gps_degrees(n(9e10)), None);
        assert!((kcal(n(541_449.0)).unwrap() - 541.449).abs() < 1e-9);
        assert_eq!(kcal(n(0.0)), None);
    }

    #[test]
    fn pace() {
        let p = Pace::from_distance_time(Meters(10_000.0), Duration::from_secs(52 * 60)).unwrap();
        assert!((p.secs_per_km() - 312.0).abs() < 1e-9);
        assert!((p.speed_kmh() - 11.538).abs() < 1e-3);
        assert_eq!(
            Pace::from_distance_time(Meters(10.0), Duration::from_secs(60)),
            None
        );
        assert_eq!(
            Pace::from_distance_time(Meters(1000.0), Duration::ZERO),
            None
        );
    }

    #[test]
    fn label_id_respects_width() {
        assert_eq!(format!("[{:<6}]", LabelId("42".into())), "[42    ]");
    }

    #[test]
    fn date_ranges() {
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        assert!(DateRange::new(d(10, 2), d(10, 1)).is_err());
        let r = DateRange::last_days(d(10, 18), 7);
        assert_eq!((r.start, r.end, r.days()), (d(10, 12), d(10, 18), 7));
        let chunks = DateRange::new(d(1, 1), d(3, 31)).unwrap().chunks(28);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].end, d(1, 28));
        assert_eq!(chunks[3].start, d(3, 26));
        assert_eq!(chunks[3].end, d(3, 31));
        assert_eq!(chunks.iter().map(DateRange::days).sum::<i64>(), 90);
    }

    proptest! {
        #[test]
        fn conversions_never_panic(v in any::<i64>(), f in any::<f64>()) {
            let _ = date_from_int(Num(Some(v as f64)));
            let _ = offset_from_quarters(Num(Some(v as f64)));
            let _ = datetime(Some(f), offset_from_quarters(Num(Some(f))));
            let _ = gps_degrees(Num(Some(f)));
            let _ = duration(Some(f));
            let _ = datetime_centi(Num(Some(f)), None);
        }
    }
}
