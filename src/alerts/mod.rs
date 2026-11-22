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

mod route_status;

pub use route_status::{
    RouteStatus, RouteStatusQuery, ServiceKind, parse_route_status, route_status_request,
};

use crate::trains::{self, StationId};
use crate::{ApiError, DecodeError, ResponseError, buses};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The Customer Alerts API's base URL. The clients use this unless given another with
/// [`ClientBuilder::alerts_base_url`](crate::client::ClientBuilder::alerts_base_url).
pub const DEFAULT_BASE_URL: &str = "https://www.transitchicago.com/api/1.0/";

/// A service that a status or alert is about: a bus route, a train route, a train
/// station, or a systemwide group.
///
/// The IDs are the same ones the other APIs use, so a [`Service::BusRoute`] can be
/// passed to [`buses`] calls and a [`Service::TrainRoute`] or [`Service::Station`] to
/// [`trains`] calls.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Service {
    /// A bus route, e.g. `"20"` or `"X9"`.
    BusRoute(buses::Route),
    /// A train ('L') line.
    TrainRoute(trains::Route),
    /// A train station (4xxxx), covering all its platforms.
    Station(StationId),
    /// A systemwide group.
    Systemwide(Systemwide),
    /// An ID that doesn't match any of the above, kept as Customer Alerts sent it.
    Other(String),
}

/// A group of services that systemwide alerts and statuses apply to.
///
/// Systemwide statuses are set separately from those of the routes they cover: an "all
/// bus routes" alert doesn't change each bus route's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Systemwide {
    /// All routes (`Systemwide`).
    AllRoutes,
    /// All bus routes (`Bus`).
    AllBusRoutes,
    /// All train routes (`Train`).
    AllTrainRoutes,
}

impl Service {
    /// Work out what kind of service an ID is, for responses that don't say.
    ///
    /// Checked in order: a systemwide group; a train line's exact code (`Brn`, not
    /// `Brown`); a 5-digit station ID starting with 4; a bus route (digits with an
    /// optional letter suffix, like `55A`, or a letter then digits, like `X9`);
    /// otherwise [`Other`](Service::Other).
    fn classify(id: &str) -> Self {
        let id = id.trim();
        if let Some(group) = Systemwide::from_id(id) {
            return Service::Systemwide(group);
        }
        if let Some(line) = trains::Route::ALL
            .into_iter()
            .find(|r| r.code().eq_ignore_ascii_case(id))
        {
            return Service::TrainRoute(line);
        }
        if id.len() == 5 && id.starts_with('4') && id.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(n) = id.parse() {
                return Service::Station(StationId(n));
            }
        }
        if is_bus_route(id) {
            return Service::BusRoute(buses::Route::from(id));
        }
        Service::Other(id.to_owned())
    }
}

impl Systemwide {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "Systemwide" => Some(Systemwide::AllRoutes),
            "Bus" => Some(Systemwide::AllBusRoutes),
            "Train" => Some(Systemwide::AllTrainRoutes),
            _ => None,
        }
    }
}

/// Digits with an optional letter suffix (`20`, `55A`), or one letter then digits with
/// an optional letter suffix (`X9`, `J14`, `N5`).
fn is_bus_route(id: &str) -> bool {
    let rest = match id.as_bytes().first() {
        Some(b) if b.is_ascii_alphabetic() => &id[1..],
        _ => id,
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && rest[digits..].bytes().all(|b| b.is_ascii_alphabetic())
}

/// Normalize a color to lowercase `"#rrggbb"`. Customer Alerts sends `"c60c30"`,
/// `"FFFFFF"`, or three-digit `"06c"`. Anything else is kept, lowercased.
fn normalize_color(color: &str) -> String {
    let hex = color.trim().trim_start_matches('#').to_ascii_lowercase();
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return hex;
    }
    match hex.len() {
        6 => format!("#{hex}"),
        3 => {
            let expanded: String = hex.chars().flat_map(|c| [c, c]).collect();
            format!("#{expanded}")
        }
        _ => hex,
    }
}

/// A query Customer Alerts would reject, caught before any request is sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum QueryError {
    /// A query by service type was given no types.
    #[error("a query by service type needs at least one type")]
    NoTypes,
    /// A query by route was given no routes.
    #[error("a query by route needs at least one route")]
    NoRoutes,
}

/// Remove duplicates, keeping the first occurrence.
fn dedup<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut out = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// Error codes that mean "nothing matched your filters" rather than a failure.
const NO_RESULTS: [u16; 2] = [25, 50];

/// Parse a Customer Alerts body whose root element is `root` (`"CTARoutes"` or
/// `"CTAAlerts"`) into `T`.
///
/// The API's JSON is converted from XML, so `ErrorCode` and `ErrorMessage` can each be
/// a string, missing, `null`, or an array with one entry per requested service type.
/// The first non-zero code is the error. Codes 25 and 50 ("no alerts/routes based on
/// your filter criteria") mean an empty result, so `T` is parsed as usual.
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
    fn classifies_service_ids() {
        use trains::Route::*;
        for (id, expected) in [
            ("Red", Service::TrainRoute(Red)),
            ("Brn", Service::TrainRoute(Brown)),
            ("G", Service::TrainRoute(Green)),
            ("Y", Service::TrainRoute(Yellow)),
            ("40380", Service::Station(StationId(40380))),
            ("20", Service::BusRoute("20".into())),
            ("206", Service::BusRoute("206".into())),
            ("55A", Service::BusRoute("55A".into())),
            ("X9", Service::BusRoute("X9".into())),
            ("J14", Service::BusRoute("J14".into())),
            ("N5", Service::BusRoute("N5".into())),
            ("Systemwide", Service::Systemwide(Systemwide::AllRoutes)),
            ("Bus", Service::Systemwide(Systemwide::AllBusRoutes)),
            ("Train", Service::Systemwide(Systemwide::AllTrainRoutes)),
        ] {
            assert_eq!(Service::classify(id), expected, "{id}");
        }
    }

    #[test]
    fn unrecognized_ids_are_other() {
        // Names aren't codes, and an unknown line like Purple Express isn't a bus route.
        // (Numbers that aren't 5-digit 4xxxx station IDs, like "4038", match the bus
        // pattern, so they're bus routes.)
        for id in ["Brown", "Pexp", "", "20-A", "XY9", "Night Owl", "4038A1"] {
            assert_eq!(
                Service::classify(id),
                Service::Other(id.to_owned()),
                "{id:?}"
            );
        }
    }

    #[test]
    fn normalizes_colors() {
        assert_eq!(normalize_color("c60c30"), "#c60c30");
        assert_eq!(normalize_color("FFFFFF"), "#ffffff");
        assert_eq!(normalize_color("06c"), "#0066cc");
        assert_eq!(normalize_color("#AbC"), "#aabbcc");
        assert_eq!(normalize_color(" 404040 "), "#404040");
        assert_eq!(normalize_color(""), "");
        assert_eq!(normalize_color("red"), "red");
        assert_eq!(normalize_color("12345"), "12345");
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
