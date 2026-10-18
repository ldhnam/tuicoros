//! Tolerant JSON building blocks for the `Raw*` types.
//!
//! The API sends numbers as JSON numbers *or* strings (`planDistance: "5000"`),
//! ids as strings *or* numbers (`userId`), and zero-fills fields that do not
//! apply. These two types accept all of that without failing, so one odd field
//! never breaks a whole response.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// A number that may arrive as a JSON number, a numeric string, or be missing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Num(pub(crate) Option<f64>);

impl Num {
    pub(crate) fn get(self) -> Option<f64> {
        self.0.filter(|v| v.is_finite())
    }

    /// `None` for missing **and** for zero: the API zero-fills fields that do not apply.
    pub(crate) fn nonzero(self) -> Option<f64> {
        self.get().filter(|v| *v != 0.0)
    }

    pub(crate) fn int(self) -> Option<i64> {
        self.get().map(|v| v.round() as i64)
    }

    /// Positive whole number (HR, cadence, power, load…); zero and negatives are "absent".
    pub(crate) fn positive_u32(self) -> Option<u32> {
        self.get()
            .filter(|v| *v > 0.0 && *v < f64::from(u32::MAX))
            .map(|v| v.round() as u32)
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Num(match Value::deserialize(d)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        }))
    }
}

/// A string that may arrive as a JSON string or number (ids), or be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FlexStr(pub(crate) Option<String>);

impl FlexStr {
    /// Trimmed, non-empty value.
    pub(crate) fn get(&self) -> Option<&str> {
        self.0.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    pub(crate) fn owned(&self) -> Option<String> {
        self.get().map(str::to_owned)
    }
}

impl<'de> Deserialize<'de> for FlexStr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(FlexStr(match Value::deserialize(d)? {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }))
    }
}

/// First `max` characters of a response body, with secrets masked. For error
/// messages and logs only.
pub(crate) fn body_snippet(body: &str, max: usize) -> String {
    let masked = mask_secrets(body);
    let mut out: String = masked.chars().take(max).collect();
    if masked.chars().count() > max {
        out.push('…');
    }
    out.replace(['\n', '\r'], " ")
}

/// Mask the string values of keys that must never reach logs.
pub(crate) fn mask_secrets(text: &str) -> String {
    const KEYS: [&str; 6] = ["accessToken", "pwd", "email", "token", "fileUrl", "headPic"];
    let mut out = text.to_owned();
    for key in KEYS {
        let pattern = format!("\"{key}\":\"");
        let mut search_from = 0;
        while let Some(found) = out[search_from..].find(&pattern) {
            let value_start = search_from + found + pattern.len();
            let Some(value_len) = out[value_start..].find('"') else {
                break;
            };
            out.replace_range(value_start..value_start + value_len, "***");
            search_from = value_start + 3;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct T {
        a: Num,
        b: Num,
        c: Num,
        d: Num,
        id: FlexStr,
        id2: FlexStr,
        id3: FlexStr,
    }

    #[test]
    fn tolerant_numbers_and_ids() {
        let t: T = serde_json::from_str(
            r#"{"a": 12.5, "b": "5000", "c": "n/a", "d": null, "id": 4211, "id2": " x9 ", "id3": ""}"#,
        )
        .unwrap();
        assert_eq!(t.a.get(), Some(12.5));
        assert_eq!(t.b.get(), Some(5000.0));
        assert_eq!(t.c.get(), None);
        assert_eq!(t.d.get(), None);
        assert_eq!(t.id.get(), Some("4211"));
        assert_eq!(t.id2.get(), Some("x9"));
        assert_eq!(t.id3.get(), None);

        let empty: T = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.a, Num(None));
    }

    #[test]
    fn zero_means_absent() {
        assert_eq!(Num(Some(0.0)).nonzero(), None);
        assert_eq!(Num(Some(0.0)).positive_u32(), None);
        assert_eq!(Num(Some(-3.0)).positive_u32(), None);
        assert_eq!(Num(Some(148.4)).positive_u32(), Some(148));
        assert_eq!(Num(Some(f64::NAN)).get(), None);
    }

    #[test]
    fn secrets_are_masked_in_snippets() {
        let body = r#"{"result":"0000","data":{"accessToken":"abc123","email":"me@x.com","pwd":"5f4d","userId":"42"}}"#;
        let s = body_snippet(body, 500);
        assert!(
            !s.contains("abc123") && !s.contains("me@x.com") && !s.contains("5f4d"),
            "{s}"
        );
        assert!(s.contains(r#""accessToken":"***""#), "{s}");
        assert!(s.contains(r#""userId":"42""#), "{s}");
    }

    #[test]
    fn snippet_truncates_on_char_boundary() {
        let s = body_snippet("ééééé", 3);
        assert_eq!(s, "ééé…");
    }
}
