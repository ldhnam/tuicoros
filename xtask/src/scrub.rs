//! Remove personal data from captured COROS responses (docs/TESTING.md §2).

use serde_json::{Map, Value};

/// Ids replaced by stable fakes: the same input always gives the same output,
/// so a `labelId` in a list capture still matches its detail capture.
const ID_KEYS: &[&str] = &[
    "userId",
    "labelId",
    "deviceId",
    "sn",
    "serialNumber",
    "authorId",
    "operateUserId",
];
const SECRET_KEYS: &[&str] = &["accessToken", "token", "pwd", "password"];
const GPS_LAT_KEYS: &[&str] = &["gpsLat", "startGpsLat", "endGpsLat", "lat", "latitude"];
const GPS_LON_KEYS: &[&str] = &[
    "gpsLon",
    "startGpsLon",
    "endGpsLon",
    "lng",
    "lon",
    "longitude",
];
/// An object with any of these keys is an activity; its `name` may reveal a
/// place ("Hanoi Morning Run"), so it is replaced.
const ACTIVITY_MARKERS: &[&str] = &["labelId", "startTimestamp", "startTime", "workoutTime"];
/// Keys whose string values name places.
const PLACE_KEYS: &[&str] = &["site", "location", "city", "address", "placeName", "region"];
/// Large chart arrays the app does not use; emptied to keep fixtures small.
const DROPPED_ARRAYS: &[&str] = &["graphList", "lapGraphList", "gpsLightDuration"];
/// Long per-second series are thinned to about this many points.
const MAX_SAMPLES: usize = 200;
/// Fake origin for GPS tracks (open sea, so nobody's home).
const FAKE_LAT: f64 = 0.5;
const FAKE_LON: f64 = -25.5;

pub fn scrub_file(path: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| format!("{path} is not JSON: {e}"))?;
    Scrubber::default().scrub(&mut value);
    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
}

#[derive(Default)]
pub struct Scrubber {
    /// First real coordinate seen, in degrees; every other point keeps its offset from it.
    origin: Option<(f64, f64)>,
}

impl Scrubber {
    pub fn scrub(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => self.scrub_object(map),
            Value::Array(items) => items.iter_mut().for_each(|v| self.scrub(v)),
            _ => {}
        }
    }

    fn scrub_object(&mut self, map: &mut Map<String, Value>) {
        self.pick_origin(map);
        let is_activity = ACTIVITY_MARKERS.iter().any(|k| map.contains_key(*k));
        for (key, value) in map.iter_mut() {
            let k = key.as_str();
            if k == "frequencyList"
                && let Value::Array(items) = value
            {
                thin(items);
            }
            if DROPPED_ARRAYS.contains(&k) && value.is_array() {
                *value = Value::Array(Vec::new());
            } else if PLACE_KEYS.contains(&k) && value.is_string() {
                *value = Value::String("Somewhere".into());
            } else if ID_KEYS.contains(&k) {
                *value = fake_id(value);
            } else if SECRET_KEYS.contains(&k) {
                *value = Value::String("SCRUBBED".into());
            } else if k == "email" || k == "account" {
                *value = Value::String("runner@example.invalid".into());
            } else if k == "nickname" || k == "realName" {
                *value = Value::String("Test Runner".into());
            } else if k == "name" && is_activity && value.is_string() {
                *value = Value::String("Activity".into());
            } else if matches!(k, "sex" | "gender") {
                *value = Value::from(0);
            } else if matches!(k, "phone" | "mobile") {
                *value = Value::String(String::new());
            } else if k == "birthday" {
                *value = Value::from(19900101);
            } else if k == "sportNote" {
                if value.as_str().is_some_and(|s| !s.is_empty()) {
                    *value = Value::String("scrubbed note".into());
                }
            } else if is_url_key(k) || value.as_str().is_some_and(|s| s.starts_with("http")) {
                if value.as_str().is_some_and(|s| !s.is_empty()) {
                    *value = Value::String("https://example.invalid/scrubbed".into());
                }
            } else if GPS_LAT_KEYS.contains(&k) {
                *value = self.shift(value, true);
            } else if GPS_LON_KEYS.contains(&k) {
                *value = self.shift(value, false);
            } else if let Some(masked) = value.as_str().and_then(mask_hardware_id) {
                *value = Value::String(masked);
            } else {
                self.scrub(value);
            }
        }
    }

    /// The first object with both a non-zero latitude and longitude sets the origin.
    fn pick_origin(&mut self, map: &Map<String, Value>) {
        if self.origin.is_some() {
            return;
        }
        let find = |keys: &[&str]| {
            keys.iter()
                .filter_map(|k| map.get(*k))
                .filter_map(degrees)
                .find(|d| *d != 0.0)
        };
        if let (Some(lat), Some(lon)) = (find(GPS_LAT_KEYS), find(GPS_LON_KEYS)) {
            self.origin = Some((lat, lon));
        }
    }

    fn shift(&self, value: &Value, is_lat: bool) -> Value {
        let (Some(raw), Some((lat0, lon0))) = (value.as_f64(), self.origin) else {
            return value.clone();
        };
        if raw == 0.0 {
            return value.clone(); // "no fix" stays "no fix"
        }
        let scaled = raw.abs() > 1000.0; // degrees ×1e7
        let deg = if scaled { raw / 1e7 } else { raw };
        let moved = if is_lat {
            deg - lat0 + FAKE_LAT
        } else {
            deg - lon0 + FAKE_LON
        };
        if scaled {
            Value::from((moved * 1e7).round() as i64)
        } else {
            Value::from(moved)
        }
    }
}

/// Device names end with part of the Bluetooth address: "COROS PACE 4 A1B2C3".
fn mask_hardware_id(s: &str) -> Option<String> {
    let (head, tail) = s.rsplit_once(' ')?;
    let is_hw = tail.len() == 6
        && tail
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
        && tail.chars().any(|c| c.is_ascii_digit());
    is_hw.then(|| format!("{head} XXXXXX"))
}

fn degrees(v: &Value) -> Option<f64> {
    let raw = v.as_f64()?;
    Some(if raw.abs() > 1000.0 { raw / 1e7 } else { raw })
}

fn is_url_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.ends_with("url") || lower == "headpic" || lower == "avatar"
}

/// Same-shaped fake: digits stay digits (same length), other strings get a prefix.
fn fake_id(value: &Value) -> Value {
    let original = match value {
        Value::String(s) if !s.is_empty() => s.clone(),
        Value::Number(n) => n.to_string(),
        other => return other.clone(),
    };
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
    for byte in original.bytes().chain(*b"tuicoros-salt") {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let all_digits = original.bytes().all(|b| b.is_ascii_digit());
    if all_digits {
        let mut digits = String::new();
        let mut h = hash;
        while digits.len() < original.len() {
            digits.push(char::from(b'0' + (h % 10) as u8));
            h = (h / 10) ^ hash.rotate_left(digits.len() as u32 * 7);
        }
        if digits.starts_with('0') {
            digits.replace_range(0..1, "9");
        }
        match value {
            Value::Number(_) => digits
                .parse::<u64>()
                .map(Value::from)
                .unwrap_or(Value::String(digits)),
            _ => Value::String(digits),
        }
    } else {
        Value::String(format!("fake-{hash:016x}"))
    }
}

fn thin(items: &mut Vec<Value>) {
    if items.len() <= MAX_SAMPLES {
        return;
    }
    let step = items.len().div_ceil(MAX_SAMPLES);
    let mut i = 0;
    items.retain(|_| {
        let keep = i % step == 0;
        i += 1;
        keep
    });
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn scrubbed(mut v: Value) -> Value {
        Scrubber::default().scrub(&mut v);
        v
    }

    #[test]
    fn personal_fields_are_replaced() {
        let out = scrubbed(json!({
            "data": {"userId": "123456789012", "email": "me@real.com", "nickname": "Nam",
                     "headPic": "https://cdn/x.png", "accessToken": "abc", "birthday": 19950607,
                     "sportFeelInfo": {"sportNote": "ran near home", "voiceNoteWavUrl": ""},
                     "imageUrl": "https://cdn/map.png", "name": "Morning Run"}
        }));
        let d = &out["data"];
        assert_ne!(d["userId"], "123456789012");
        assert_eq!(d["userId"].as_str().unwrap().len(), 12);
        assert_eq!(d["email"], "runner@example.invalid");
        assert_eq!(d["nickname"], "Test Runner");
        assert_eq!(d["headPic"], "https://example.invalid/scrubbed");
        assert_eq!(d["imageUrl"], "https://example.invalid/scrubbed");
        assert_eq!(d["accessToken"], "SCRUBBED");
        assert_eq!(d["birthday"], 19900101);
        assert_eq!(d["sportFeelInfo"]["sportNote"], "scrubbed note");
        assert_eq!(
            d["sportFeelInfo"]["voiceNoteWavUrl"], "",
            "empty stays empty"
        );
        assert_eq!(d["name"], "Morning Run", "non-activity names are kept");
        let a = scrubbed(
            json!({"labelId": "1", "name": "Hanoi Run", "userInfo": {"avatar": "https://x/y.png", "sex": 1}}),
        );
        assert_eq!(a["name"], "Activity", "activity names can reveal places");
        assert_eq!(a["userInfo"]["avatar"], "https://example.invalid/scrubbed");
        assert_eq!(a["userInfo"]["sex"], 0);
        let r = scrubbed(json!({"recordDetailList": [{"site": "Hanoi Run", "graphList": [1, 2]}]}));
        assert_eq!(r["recordDetailList"][0]["site"], "Somewhere");
        assert_eq!(r["recordDetailList"][0]["graphList"], json!([]));
        let dev = scrubbed(
            json!({"deviceList": [{"name": "COROS PACE 4 A1B2C3"}], "device": "COROS PACE 4"}),
        );
        assert_eq!(dev["deviceList"][0]["name"], "COROS PACE 4 XXXXXX");
        assert_eq!(dev["device"], "COROS PACE 4");
        let w = scrubbed(json!({"exercises": [{"name": "T3001"}], "name": "Interval"}));
        assert_eq!(
            (w["name"].as_str(), w["exercises"][0]["name"].as_str()),
            (Some("Interval"), Some("T3001"))
        );
    }

    #[test]
    fn ids_are_stable_and_keep_their_type() {
        let a = scrubbed(json!({"labelId": "476100000000000001"}));
        let b = scrubbed(json!({"x": [{"labelId": "476100000000000001"}]}));
        assert_eq!(a["labelId"], b["x"][0]["labelId"]);
        let n = scrubbed(json!({"userId": 123456789012_u64}));
        assert!(n["userId"].is_u64());
        assert_ne!(n["userId"], 123456789012_u64);
        let s = scrubbed(json!({"deviceId": "AB-12"}));
        assert!(s["deviceId"].as_str().unwrap().starts_with("fake-"));
    }

    #[test]
    fn gps_track_keeps_its_shape_but_moves() {
        let out = scrubbed(json!({"frequencyList": [
            {"gpsLat": 210285000, "gpsLon": 1058542000},
            {"gpsLat": 210286000, "gpsLon": 1058540000},
            {"gpsLat": 0, "gpsLon": 0}
        ], "lapList": [{"startGpsLat": 210285500, "startGpsLon": 1058541000}]}));
        let f = &out["frequencyList"];
        assert_eq!(f[0]["gpsLat"], (FAKE_LAT * 1e7) as i64);
        assert_eq!(f[0]["gpsLon"], (FAKE_LON * 1e7) as i64);
        let dlat = f[1]["gpsLat"].as_i64().unwrap() - f[0]["gpsLat"].as_i64().unwrap();
        let dlon = f[1]["gpsLon"].as_i64().unwrap() - f[0]["gpsLon"].as_i64().unwrap();
        assert_eq!((dlat, dlon), (1000, -2000), "relative movement preserved");
        assert_eq!(f[2]["gpsLat"], 0, "no-fix samples stay zero");
        assert_eq!(
            out["lapList"][0]["startGpsLat"],
            (FAKE_LAT * 1e7) as i64 + 500
        );
    }

    #[test]
    fn long_series_are_thinned() {
        let samples: Vec<Value> = (0..1000).map(|i| json!({"timestamp": i})).collect();
        let out = scrubbed(json!({"frequencyList": samples}));
        let n = out["frequencyList"].as_array().unwrap().len();
        assert!((MAX_SAMPLES / 2..=MAX_SAMPLES).contains(&n), "{n}");
        assert_eq!(out["frequencyList"][0]["timestamp"], 0);
    }
}
