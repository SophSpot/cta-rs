//! The Customer Alerts API: service status and alerts for CTA buses, trains, and
//! stations.
//!
//! This API needs no key. Its requests go to [`DEFAULT_BASE_URL`] unless the client is
//! given another with
//! [`ClientBuilder::alerts_base_url`](crate::client::ClientBuilder::alerts_base_url).
//!
//! Most users call the methods on [`AsyncClient::alerts`](crate::AsyncClient::alerts) or
//! [`BlockingClient::alerts`](crate::BlockingClient::alerts). The `*_request` and
//! `parse_*` functions are for sending requests yourself: they build the URL and parse the
//! response body without doing any I/O.

use crate::{ApiError, DecodeError, ResponseError};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The Customer Alerts API's base URL. The clients use this unless given another with
/// [`ClientBuilder::alerts_base_url`](crate::client::ClientBuilder::alerts_base_url).
pub const DEFAULT_BASE_URL: &str = "https://www.transitchicago.com/api/1.0/";

/// Error codes that mean "nothing matched your filters" rather than a failure.
const NO_RESULTS: [u16; 2] = [25, 50];

/// Parse a Customer Alerts body whose root element is `root` (`"CTARoutes"` or
/// `"CTAAlerts"`) into `T`.
///
/// The API's JSON is converted from XML, so `ErrorCode` and `ErrorMessage` can each be
/// a string, missing, `null`, or an array with one entry per requested service type.
/// The first non-zero code is the error. Codes 25 and 50 ("no alerts/routes based on
/// your filter criteria") mean an empty result, so `T` is parsed as usual.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used once alerts endpoints land")
)]
pub(crate) fn parse_envelope<T: DeserializeOwned>(
    body: &[u8],
    root: &str,
) -> Result<T, ResponseError> {
    let mut top: Map<String, Value> = serde_json::from_slice(body).map_err(DecodeError::from)?;
    let mut response = match top.remove(root) {
        Some(Value::Object(map)) => map,
        _ => return Err(DecodeError(format!("missing {root} in response").into()).into()),
    };
    let codes = values(response.remove("ErrorCode"));
    let messages = values(response.remove("ErrorMessage"));
    for (i, code) in codes.iter().enumerate() {
        let code = parse_code(code)?;
        if code == 0 || NO_RESULTS.contains(&code) {
            continue;
        }
        let message = messages
            .get(i)
            .or(messages.first())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        return Err(ApiError::alerts(code, message).into());
    }
    Ok(serde_json::from_value(Value::Object(response)).map_err(DecodeError::from)?)
}

/// A field that may be missing, `null`, one value, or an array of values.
fn values(v: Option<Value>) -> Vec<Value> {
    match v {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items,
        Some(other) => vec![other],
    }
}

/// An error code sent as a string or a number; `null` or `""` means 0.
fn parse_code(v: &Value) -> Result<u16, DecodeError> {
    let bad = || DecodeError(format!("not an error code: {v}").into());
    match v {
        Value::Null => Ok(0),
        Value::Number(n) => n
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(bad),
        Value::String(s) if s.trim().is_empty() => Ok(0),
        Value::String(s) => s.trim().parse().map_err(|_| bad()),
        _ => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Api, ApiErrorKind};
    use serde::Deserialize;

    const ROUTES: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes.json");
    const SYSTEMWIDE: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes_systemwide.json");
    const ALERTS: &[u8] = include_bytes!("../../tests/fixtures/alerts_alerts.json");
    const NO_ALERTS: &[u8] = include_bytes!("../../tests/fixtures/alerts_no_results.json");
    const NO_ROUTES: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes_no_results.json");
    const BAD_PARAM: &[u8] = include_bytes!("../../tests/fixtures/alerts_error_500.json");

    /// Stand-ins for the endpoints' results.
    #[derive(Debug, Deserialize)]
    struct Routes {
        #[serde(default, rename = "RouteInfo")]
        route_info: Value,
    }

    #[derive(Debug, Deserialize)]
    struct Alerts {
        #[serde(default, rename = "Alert")]
        alert: Value,
    }

    fn count(v: &Value) -> usize {
        match v {
            Value::Null => 0,
            Value::Array(a) => a.len(),
            _ => 1,
        }
    }

    #[test]
    fn array_error_code_of_zeros_is_success() {
        // ErrorCode is ["0","0"] when several service types are returned.
        let r: Routes = parse_envelope(ROUTES, "CTARoutes").unwrap();
        assert_eq!(count(&r.route_info), 138);
    }

    #[test]
    fn missing_error_code_is_success() {
        let r: Routes = parse_envelope(SYSTEMWIDE, "CTARoutes").unwrap();
        assert_eq!(count(&r.route_info), 3);
    }

    #[test]
    fn string_error_code_of_zero_is_success() {
        let a: Alerts = parse_envelope(ALERTS, "CTAAlerts").unwrap();
        assert_eq!(count(&a.alert), 53);
    }

    #[test]
    fn no_results_codes_are_empty() {
        let a: Alerts = parse_envelope(NO_ALERTS, "CTAAlerts").unwrap();
        assert_eq!(count(&a.alert), 0);
        let r: Routes = parse_envelope(NO_ROUTES, "CTARoutes").unwrap();
        assert_eq!(count(&r.route_info), 0);
        let body =
            br#"{"CTAAlerts": {"ErrorCode": "25", "ErrorMessage": "There are no active alerts"}}"#;
        let a: Alerts = parse_envelope(body, "CTAAlerts").unwrap();
        assert_eq!(count(&a.alert), 0);
    }

    #[test]
    fn other_codes_are_api_errors() {
        match parse_envelope::<Routes>(BAD_PARAM, "CTARoutes") {
            Err(ResponseError::Api(e)) => {
                assert_eq!(e.api, Api::Alerts);
                assert_eq!(e.code, Some(500));
                assert_eq!(e.message, "Invalid parameter: 'bogus'");
                assert_eq!(e.kind(), ApiErrorKind::Other);
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
        let body = br#"{"CTARoutes": {"ErrorCode": "900", "ErrorMessage": "Server Error"}}"#;
        assert!(matches!(
            parse_envelope::<Routes>(body, "CTARoutes"),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::ServerError
        ));
    }

    #[test]
    fn first_non_zero_code_in_an_array_wins() {
        let body = br#"{"CTARoutes": {
            "ErrorCode": ["0", "900", "101"],
            "ErrorMessage": [null, "Server Error", "Invalid type"]
        }}"#;
        match parse_envelope::<Routes>(body, "CTARoutes") {
            Err(ResponseError::Api(e)) => {
                assert_eq!(e.code, Some(900));
                assert_eq!(e.message, "Server Error");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
        // A no-results code next to a real error doesn't hide the error.
        let body =
            br#"{"CTARoutes": {"ErrorCode": ["50", "101"], "ErrorMessage": "Invalid type"}}"#;
        assert!(matches!(
            parse_envelope::<Routes>(body, "CTARoutes"),
            Err(ResponseError::Api(e)) if e.code == Some(101) && e.message == "Invalid type"
        ));
    }

    #[test]
    fn numeric_and_empty_codes() {
        let body = br#"{"CTAAlerts": {"ErrorCode": 0, "ErrorMessage": ""}}"#;
        assert!(parse_envelope::<Alerts>(body, "CTAAlerts").is_ok());
        let body = br#"{"CTAAlerts": {"ErrorCode": "", "ErrorMessage": null}}"#;
        assert!(parse_envelope::<Alerts>(body, "CTAAlerts").is_ok());
    }

    #[test]
    fn garbage_is_decode_error() {
        for body in [
            &b"not json"[..],
            b"{}",
            br#"{"CTARoutes": {}}"#, // wrong root for alerts
            br#"{"CTAAlerts": {"ErrorCode": "abc"}}"#,
            // What an unknown parameter gets, despite outputType=JSON.
            br#"<?xml version="1.0" encoding="utf-8"?><CTAAlerts><ErrorCode>500</ErrorCode></CTAAlerts>"#,
        ] {
            assert!(
                matches!(
                    parse_envelope::<Alerts>(body, "CTAAlerts"),
                    Err(ResponseError::Decode(_))
                ),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }
}
