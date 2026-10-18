//! Display formatting (docs/UI_DESIGN.md §5). Metric only for now.

use std::time::Duration;

use chrono::{Local, NaiveDate};
use coros_api::{Meters, Pace, SportCategory};

pub const MISSING: &str = "—";

/// `10.02 km`, or `850 m` under 1 km.
pub fn distance(d: Option<Meters>) -> String {
    match d {
        Some(Meters(m)) if m >= 1000.0 => format!("{:.2} km", m / 1000.0),
        Some(Meters(m)) => format!("{m:.0} m"),
        None => MISSING.to_owned(),
    }
}

/// `52:11` or `1:05:12`.
pub fn duration(d: Option<Duration>) -> String {
    let Some(d) = d else {
        return MISSING.to_owned();
    };
    let total = d.as_secs_f64().round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `m:ss` from seconds, rounding to whole seconds without producing `:60`.
fn min_sec(secs: f64) -> String {
    let total = secs.round() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Pace or speed, the way each sport is usually shown.
pub fn pace(p: Option<Pace>, category: SportCategory) -> String {
    let Some(p) = p else {
        return MISSING.to_owned();
    };
    match category {
        SportCategory::Run | SportCategory::Walk => format!("{} /km", min_sec(p.secs_per_km())),
        SportCategory::Bike => format!("{:.1} km/h", p.speed_kmh()),
        SportCategory::Swim => format!("{} /100m", min_sec(p.secs_per_km() / 10.0)),
        SportCategory::Strength | SportCategory::Other => MISSING.to_owned(),
    }
}

pub fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| MISSING.to_owned(), |v| v.to_string())
}

pub fn opt_f(v: Option<f64>, decimals: usize) -> String {
    v.map_or_else(|| MISSING.to_owned(), |v| format!("{v:.decimals$}"))
}

/// Pad or cut `s` to exactly `width` characters.
pub fn fit(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count <= width {
        format!("{s:<width$}")
    } else {
        let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Parse a CLI date: `YYYY-MM-DD`, `YYYYMMDD`, `today`, `yesterday`, or `-N` (N days ago).
pub fn parse_date(s: &str) -> Result<NaiveDate, String> {
    parse_date_relative_to(s, Local::now().date_naive())
}

fn parse_date_relative_to(s: &str, today: NaiveDate) -> Result<NaiveDate, String> {
    let s = s.trim();
    match s {
        "today" => return Ok(today),
        "yesterday" => return Ok(today - chrono::Duration::days(1)),
        _ => {}
    }
    if let Some(days) = s.strip_prefix('-').and_then(|n| n.parse::<i64>().ok()) {
        return Ok(today - chrono::Duration::days(days));
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%Y%m%d"))
        .map_err(|_| format!("invalid date `{s}` (use YYYY-MM-DD, today, yesterday or -N)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(distance(Some(Meters(10_020.5))), "10.02 km");
        assert_eq!(distance(Some(Meters(850.4))), "850 m");
        assert_eq!(distance(None), "—");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(Some(Duration::from_secs(3131))), "52:11");
        assert_eq!(duration(Some(Duration::from_secs(3912))), "1:05:12");
        assert_eq!(duration(Some(Duration::from_secs_f64(59.6))), "1:00");
        assert_eq!(duration(None), "—");
    }

    #[test]
    fn paces_by_sport() {
        let p = Some(Pace(312.4));
        assert_eq!(pace(p, SportCategory::Run), "5:12 /km");
        assert_eq!(
            pace(Some(Pace(299.7)), SportCategory::Run),
            "5:00 /km",
            "no 4:60"
        );
        assert_eq!(pace(Some(Pace(144.6)), SportCategory::Bike), "24.9 km/h");
        assert_eq!(pace(Some(Pace(1120.0)), SportCategory::Swim), "1:52 /100m");
        assert_eq!(pace(p, SportCategory::Strength), "—");
    }

    #[test]
    fn fitting_text() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
    }

    #[test]
    fn date_parsing() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let d = |s| parse_date_relative_to(s, today);
        assert_eq!(
            d("2026-09-01"),
            Ok(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
        );
        assert_eq!(
            d("20260901"),
            Ok(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
        );
        assert_eq!(d("today"), Ok(today));
        assert_eq!(
            d("yesterday"),
            Ok(NaiveDate::from_ymd_opt(2026, 10, 17).unwrap())
        );
        assert_eq!(d("-30"), Ok(NaiveDate::from_ymd_opt(2026, 9, 18).unwrap()));
        assert!(d("18/10/2026").is_err());
    }
}
