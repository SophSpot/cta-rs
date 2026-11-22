//! The Bus Tracker API.
//!
//! Requests need a Bus Tracker key, set with
//! [`ClientBuilder::bus_key`](crate::client::ClientBuilder::bus_key).
//!
//! Most users call the methods on [`AsyncClient::buses`](crate::AsyncClient::buses) or
//! [`BlockingClient::buses`](crate::BlockingClient::buses). The `*_request` and `parse_*`
//! functions are for sending requests yourself: they build the URL and parse the response
//! body without doing any I/O.

mod routes;
mod time;

pub use routes::{RouteInfo, parse_routes, routes_request};
pub use time::{parse_time, time_request};

use crate::deserializers::one_or_many;
use crate::{ApiError, DecodeError, ResponseError};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fmt::{Display, Formatter};

/// The Bus Tracker API's base URL. The clients use this unless given another with
/// [`ClientBuilder::bus_base_url`](crate::client::ClientBuilder::bus_base_url).
pub const DEFAULT_BASE_URL: &str = "https://www.ctabustracker.com/bustime/api/v3/";

/// A bus route's code, such as `"20"` (Madison) or `"X9"` (Ashland Express).
///
/// This is the `rt` that Bus Tracker calls take and return. List every route with its
/// name using [`Buses::routes`](crate::client::Buses::routes).
///
/// The code is used exactly as given, without checking it. An unknown route gets no
/// results rather than an error.
///
/// ```
/// use cta::buses::Route;
///
/// let route = Route::from("X9");
/// assert_eq!(route, Route::new("X9"));
/// assert_eq!(route.as_str(), "X9");
/// assert_eq!(route.to_string(), "X9");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct Route(pub String);

impl Route {
    /// A route with this code.
    pub fn new(code: impl Into<String>) -> Self {
        Route(code.into())
    }

    /// The route's code, e.g. `"20"`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Route {
    fn from(code: &str) -> Self {
        Route::new(code)
    }
}

impl From<String> for Route {
    fn from(code: String) -> Self {
        Route(code)
    }
}

impl Display for Route {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // `pad` (not `write_str`) so `{:<4}` / `{:>4}` alignment works in tables
        f.pad(&self.0)
    }
}

#[derive(Deserialize)]
struct RawResponse {
    #[serde(rename = "bustime-response")]
    response: Map<String, Value>,
}

/// One entry in a response's `error` array.
#[derive(Deserialize)]
struct RawError {
    msg: String,
    /// The parameter the error is about, e.g. `{"stpid": "456"}`. Empty for errors
    /// about the whole request, such as an invalid key.
    #[serde(flatten)]
    context: Map<String, Value>,
}

/// Parse a `bustime-response` body into `T`.
///
/// - An error about the whole request (one with only `msg`) is returned as an
///   [`ApiError`], even if the response also has results.
/// - Errors about one requested ID (`stpid`, `vid`, `rt`, `pid`, ...) are skipped: that ID
///   has no results. Bus Tracker reports unknown IDs and IDs with no service the same way.
/// - The errors are checked before `T` is parsed, so an error response without results
///   is an [`ApiError`], not a [`DecodeError`].
pub(crate) fn parse_envelope<T: DeserializeOwned>(body: &[u8]) -> Result<T, ResponseError> {
    let mut response = serde_json::from_slice::<RawResponse>(body)
        .map_err(DecodeError::from)?
        .response;

    if let Some(errors) = response.remove("error") {
        let errors: Vec<RawError> = one_or_many(errors).map_err(DecodeError::from)?;
        if let Some(e) = errors.into_iter().find(|e| e.context.is_empty()) {
            return Err(ApiError::bus(e.msg).into());
        }
    }

    Ok(serde_json::from_value(Value::Object(response)).map_err(DecodeError::from)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Api, ApiErrorKind};

    /// A stand-in for an endpoint's results.
    #[derive(Debug, Deserialize)]
    struct Prds {
        #[serde(default)]
        prd: Vec<Value>,
    }

    /// A stand-in for an endpoint with a required field, like `gettime`'s `tm`.
    #[derive(Debug, Deserialize)]
    struct Time {
        #[allow(dead_code)]
        tm: String,
    }

    const BAD_KEY: &[u8] =
        br#"{"bustime-response": {"error": [{"msg": "Invalid API access key supplied"}]}}"#;

    #[test]
    fn route_conversions_agree() {
        let from_str = Route::from("X9");
        assert_eq!(from_str, Route::from(String::from("X9")));
        assert_eq!(from_str, Route::new("X9"));
        assert_eq!(from_str, Route("X9".into()));
    }

    #[test]
    fn route_is_kept_as_given() {
        assert_eq!(Route::from(" x9 ").as_str(), " x9 ");
    }

    #[test]
    fn route_display_supports_padding() {
        assert_eq!(format!("[{:<4}]", Route::from("20")), "[20  ]");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn route_serializes_as_plain_string() {
        assert_eq!(
            serde_json::to_string(&Route::from("X9")).unwrap(),
            r#""X9""#
        );
        assert_eq!(
            serde_json::from_str::<Route>(r#""X9""#).unwrap(),
            Route::from("X9")
        );
    }

    #[test]
    fn request_error_becomes_api_error() {
        match parse_envelope::<Prds>(BAD_KEY) {
            Err(ResponseError::Api(e)) => {
                assert_eq!(e.api, Api::Bus);
                assert_eq!(e.code, None);
                assert_eq!(e.kind(), ApiErrorKind::InvalidKey);
                assert_eq!(e.message, "Invalid API access key supplied");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[test]
    fn request_error_wins_over_missing_required_fields() {
        assert!(matches!(
            parse_envelope::<Time>(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[test]
    fn per_id_errors_are_skipped() {
        let body = br#"{"bustime-response": {
            "prd": [{"stpid": "456"}, {"stpid": "456"}],
            "error": [{"stpid": "sad", "msg": "No data found for parameter"}]
        }}"#;
        assert_eq!(parse_envelope::<Prds>(body).unwrap().prd.len(), 2);
    }

    #[test]
    fn only_per_id_errors_is_empty() {
        let body = br#"{"bustime-response": {
            "error": [{"stpid": "99999999", "msg": "No data found for parameter"}]
        }}"#;
        assert!(parse_envelope::<Prds>(body).unwrap().prd.is_empty());
    }

    #[test]
    fn request_error_wins_over_results_and_per_id_errors() {
        let body = br#"{"bustime-response": {
            "prd": [{"stpid": "456"}],
            "error": [
                {"stpid": "sad", "msg": "No data found for parameter"},
                {"msg": "Internal server error - Unable to complete request at this time"}
            ]
        }}"#;
        assert!(matches!(
            parse_envelope::<Prds>(body),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::ServerError
        ));
    }

    #[test]
    fn single_error_object_is_accepted() {
        let body =
            br#"{"bustime-response": {"error": {"msg": "Invalid API access key supplied"}}}"#;
        assert!(matches!(
            parse_envelope::<Prds>(body),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[test]
    fn empty_response_is_empty() {
        let body = br#"{"bustime-response": {}}"#;
        assert!(parse_envelope::<Prds>(body).unwrap().prd.is_empty());
    }

    #[test]
    fn garbage_is_decode_error() {
        for body in [
            &b"not json"[..],
            b"{}",
            br#"{"ctatt": {"errCd": "0"}}"#,
            b"<html>oops</html>",
        ] {
            assert!(
                matches!(parse_envelope::<Prds>(body), Err(ResponseError::Decode(_))),
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn error_without_msg_is_decode_error() {
        let body = br#"{"bustime-response": {"error": [{"stpid": "456"}]}}"#;
        assert!(matches!(
            parse_envelope::<Prds>(body),
            Err(ResponseError::Decode(_))
        ));
    }
}
