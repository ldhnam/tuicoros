//! Unwrapping `{ result, message, apiCode, data }` (docs/COROS_API.md §2).

use serde::Deserialize;
use serde_json::Value;

use crate::error::{CorosError, Result};
use crate::region::Region;
use crate::util::{FlexStr, body_snippet};

pub(crate) const SUCCESS: &str = "0000";
pub(crate) const TOKEN_INVALID: &str = "1019";
const SNIPPET_LEN: usize = 300;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    #[serde(default)]
    result: FlexStr,
    #[serde(default)]
    message: FlexStr,
    #[serde(default)]
    tlog_id: FlexStr,
    data: Option<Value>,
}

/// Turn a status + body into the envelope's `data` (which may be absent),
/// or a typed error.
pub(crate) fn decode(
    endpoint: &str,
    status: u16,
    body: &str,
    region: Region,
) -> Result<Option<Value>> {
    if status == 429 {
        return Err(CorosError::RateLimited);
    }
    let envelope: Envelope = match serde_json::from_str(body) {
        Ok(envelope) => envelope,
        Err(err) => {
            return Err(if (200..300).contains(&status) {
                CorosError::Unexpected {
                    endpoint: endpoint.to_owned(),
                    detail: format!("not JSON ({err})"),
                    body_snippet: body_snippet(body, SNIPPET_LEN),
                }
            } else {
                CorosError::Http {
                    endpoint: endpoint.to_owned(),
                    status,
                    body_snippet: body_snippet(body, SNIPPET_LEN),
                }
            });
        }
    };

    let Some(result) = envelope.result.get() else {
        return Err(if (200..300).contains(&status) {
            CorosError::Unexpected {
                endpoint: endpoint.to_owned(),
                detail: "no `result` field: not a COROS API response".to_owned(),
                body_snippet: body_snippet(body, SNIPPET_LEN),
            }
        } else {
            CorosError::Http {
                endpoint: endpoint.to_owned(),
                status,
                body_snippet: body_snippet(body, SNIPPET_LEN),
            }
        });
    };

    if result == TOKEN_INVALID {
        return Err(CorosError::TokenInvalid { region });
    }
    if result != SUCCESS {
        return Err(CorosError::Api {
            endpoint: endpoint.to_owned(),
            result: result.to_owned(),
            message: envelope
                .message
                .owned()
                .unwrap_or_else(|| "(no message)".to_owned()),
            tlog_id: envelope.tlog_id.owned(),
        });
    }
    if !(200..300).contains(&status) {
        return Err(CorosError::Http {
            endpoint: endpoint.to_owned(),
            status,
            body_snippet: body_snippet(body, SNIPPET_LEN),
        });
    }
    Ok(envelope.data.filter(|d| !d.is_null()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_us(status: u16, body: &str) -> Result<Option<Value>> {
        decode("test/endpoint", status, body, Region::Us)
    }

    #[test]
    fn success_with_and_without_data() {
        let data = decode_us(200, r#"{"result":"0000","message":"OK","data":{"a":1}}"#).unwrap();
        assert_eq!(data, Some(serde_json::json!({"a": 1})));
        let none = decode_us(
            200,
            r#"{"apiCode":"8D2DB17A","message":"OK","result":"0000"}"#,
        )
        .unwrap();
        assert_eq!(none, None);
    }

    #[test]
    fn token_invalid() {
        let err = decode_us(
            200,
            r#"{"result":"1019","message":"Access token is invalid"}"#,
        )
        .unwrap_err();
        assert!(err.is_token_invalid());
        assert_eq!(err.result_code(), Some("1019"));
    }

    #[test]
    fn api_error_keeps_code_message_and_trace_id() {
        let err = decode_us(
            200,
            r#"{"result":"1001","message":"Service exceptions","tlogId":"t-9"}"#,
        )
        .unwrap_err();
        match err {
            CorosError::Api {
                result,
                message,
                tlog_id,
                endpoint,
            } => {
                assert_eq!(
                    (result.as_str(), message.as_str()),
                    ("1001", "Service exceptions")
                );
                assert_eq!(tlog_id.as_deref(), Some("t-9"));
                assert_eq!(endpoint, "test/endpoint");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn numeric_result_code_is_accepted() {
        assert!(
            decode_us(200, r#"{"result":1019}"#)
                .unwrap_err()
                .is_token_invalid()
        );
    }

    #[test]
    fn rate_limit_and_html_bodies() {
        assert!(matches!(
            decode_us(429, "<html>Too Many</html>"),
            Err(CorosError::RateLimited)
        ));
        let err = decode_us(502, "<html>Bad Gateway</html>").unwrap_err();
        assert!(matches!(err, CorosError::Http { status: 502, .. }));
        assert!(err.is_retryable());
        let err = decode_us(200, "<html>oops</html>").unwrap_err();
        assert!(matches!(err, CorosError::Unexpected { .. }));
        assert!(!err.is_retryable());
    }

    #[test]
    fn non_2xx_with_success_envelope_is_http_error() {
        let err = decode_us(500, r#"{"result":"0000","data":{}}"#).unwrap_err();
        assert!(matches!(err, CorosError::Http { status: 500, .. }));
    }

    #[test]
    fn json_without_result_is_unexpected() {
        assert!(matches!(
            decode_us(200, r#"{"foo":1}"#),
            Err(CorosError::Unexpected { .. })
        ));
        assert!(matches!(
            decode_us(200, "[1,2]"),
            Err(CorosError::Unexpected { .. })
        ));
    }
}
