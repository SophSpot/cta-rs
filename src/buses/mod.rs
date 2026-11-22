//! The Bus Tracker API.
//!
//! Requests need a Bus Tracker key, set with
//! [`ClientBuilder::bus_key`](crate::client::ClientBuilder::bus_key).
//!
//! Most users call the methods on [`AsyncClient::buses`](crate::AsyncClient::buses) or
//! [`BlockingClient::buses`](crate::BlockingClient::buses). The `*_request` and `parse_*`
//! functions are for sending requests yourself: they build the URL and parse the response
//! body without doing any I/O.

mod directions;
mod locales;
mod routes;
mod stops;
mod time;

pub use directions::{DirectionInfo, DirectionsQuery, directions_request, parse_directions};
pub use locales::{LocaleInfo, LocalesQuery, locales_request, parse_locales};
pub use routes::{RouteInfo, RoutesQuery, parse_routes, routes_request};
pub use stops::{Stop, StopsQuery, parse_stops, stops_request};
pub use time::{parse_time, time_request};

use crate::deserializers::one_or_many;
use crate::{ApiError, DecodeError, ResponseError};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::fmt::{Display, Formatter};
use url::Url;

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

/// The direction a route runs in, such as [`Direction::Eastbound`].
///
/// List a route's directions with [`Buses::directions`](crate::client::Buses::directions).
/// Every CTA route uses the four compass directions; [`Other`](Direction::Other) keeps
/// anything else exactly as Bus Tracker sent it.
///
/// Converting from text ignores case, so `"eastbound"` and `"EASTBOUND"` both become
/// [`Direction::Eastbound`]. Requests always send the form CTA uses, e.g. `"Eastbound"`.
///
/// ```
/// use cta::buses::Direction;
///
/// assert_eq!(Direction::from("EASTBOUND"), Direction::Eastbound);
/// assert_eq!(Direction::Eastbound.as_str(), "Eastbound");
/// assert_eq!(Direction::from("Clockwise"), Direction::Other("Clockwise".into()));
/// ```
///
/// Build an `Other` with `From` rather than directly: `Direction::Other("Eastbound".into())`
/// is not equal to [`Direction::Eastbound`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(from = "String", into = "String")
)]
#[non_exhaustive]
pub enum Direction {
    /// Northbound.
    Northbound,
    /// Southbound.
    Southbound,
    /// Eastbound.
    Eastbound,
    /// Westbound.
    Westbound,
    /// A direction other than the four above, as Bus Tracker sent it.
    Other(String),
}

impl Direction {
    /// The text Bus Tracker uses for this direction, e.g. `"Eastbound"`.
    pub fn as_str(&self) -> &str {
        match self {
            Direction::Northbound => "Northbound",
            Direction::Southbound => "Southbound",
            Direction::Eastbound => "Eastbound",
            Direction::Westbound => "Westbound",
            Direction::Other(s) => s,
        }
    }

    /// The named direction matching `s`, ignoring case.
    fn named(s: &str) -> Option<Self> {
        [
            Direction::Northbound,
            Direction::Southbound,
            Direction::Eastbound,
            Direction::Westbound,
        ]
        .into_iter()
        .find(|d| d.as_str().eq_ignore_ascii_case(s))
    }
}

impl From<&str> for Direction {
    fn from(s: &str) -> Self {
        Direction::named(s).unwrap_or_else(|| Direction::Other(s.to_owned()))
    }
}

impl From<String> for Direction {
    fn from(s: String) -> Self {
        Direction::named(&s).unwrap_or(Direction::Other(s))
    }
}

impl From<Direction> for String {
    fn from(d: Direction) -> Self {
        match d {
            Direction::Other(s) => s,
            named => named.as_str().to_owned(),
        }
    }
}

impl Display for Direction {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

/// A Bus Tracker language code, such as `"en"` or `"es"`.
///
/// Pass it to a query's `locale` method. List the codes Bus Tracker accepts with
/// [`Buses::locales`](crate::client::Buses::locales).
///
/// CTA lists Spanish, but its route, direction, and stop text is English in every
/// locale today. Only the language names from `locales` are translated. Bus Tracker
/// uses its default language for a code it doesn't support, without saying so.
///
/// ```
/// use cta::buses::Locale;
///
/// assert_eq!(Locale::from("es").as_str(), "es");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct Locale(pub String);

impl Locale {
    /// A locale with this code.
    pub fn new(code: impl Into<String>) -> Self {
        Locale(code.into())
    }

    /// The locale's code, e.g. `"es"`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Locale {
    fn from(code: &str) -> Self {
        Locale::new(code)
    }
}

impl From<String> for Locale {
    fn from(code: String) -> Self {
        Locale(code)
    }
}

impl Display for Locale {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.pad(&self.0)
    }
}

/// A bus stop ID (`stpid`), such as `456` for Madison & Jefferson.
///
/// Bus stop IDs are a different set from Train Tracker's
/// [`trains::StopId`](crate::trains::StopId).
///
/// ```
/// use cta::buses::StopId;
///
/// assert_eq!(StopId(456).to_string(), "456");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StopId(pub u32);

impl Display for StopId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

/// A detour's ID, such as `"8A3D8B0D-D95E-4621-BAF8-E969867256B5"`.
///
/// Stops list the detours that add or remove them, and the same ID identifies the
/// detour elsewhere in Bus Tracker. The ID is kept exactly as Bus Tracker sent it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
pub struct DetourId(pub String);

impl DetourId {
    /// A detour ID with this text.
    pub fn new(id: impl Into<String>) -> Self {
        DetourId(id.into())
    }

    /// The ID's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for DetourId {
    fn from(id: &str) -> Self {
        DetourId::new(id)
    }
}

impl From<String> for DetourId {
    fn from(id: String) -> Self {
        DetourId(id)
    }
}

impl Display for DetourId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.pad(&self.0)
    }
}

/// A query Bus Tracker would reject, caught before any request is sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum QueryError {
    /// A stops query was given no stop IDs.
    #[error("a stops query needs at least one stop ID")]
    NoStops,
    /// More stop IDs than Bus Tracker accepts in one request.
    #[error("too many stops: {0} (Bus Tracker accepts at most 10)")]
    TooManyStops(usize),
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

/// Join IDs with commas, e.g. `456,4727`.
fn join<T: Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Add `locale=<code>` to `url` if a locale was chosen.
fn append_locale(url: &mut Url, locale: Option<&Locale>) {
    if let Some(locale) = locale {
        url.query_pairs_mut().append_pair("locale", locale.as_str());
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

    #[test]
    fn direction_parsing_ignores_case() {
        for s in ["Eastbound", "eastbound", "EASTBOUND", "eAsTbOuNd"] {
            assert_eq!(Direction::from(s), Direction::Eastbound, "{s}");
            assert_eq!(Direction::from(s.to_owned()), Direction::Eastbound, "{s}");
        }
        assert_eq!(Direction::from("Northbound"), Direction::Northbound);
        assert_eq!(Direction::from("Southbound"), Direction::Southbound);
        assert_eq!(Direction::from("Westbound"), Direction::Westbound);
    }

    #[test]
    fn direction_other_is_kept_as_given() {
        for s in ["Clockwise", " Eastbound", "East", ""] {
            let d = Direction::from(s);
            assert_eq!(d, Direction::Other(s.into()), "{s:?}");
            assert_eq!(d.as_str(), s);
            assert_eq!(d.to_string(), s);
        }
    }

    #[test]
    fn direction_sends_canonical_text() {
        assert_eq!(Direction::from("EASTBOUND").as_str(), "Eastbound");
        assert_eq!(Direction::Westbound.to_string(), "Westbound");
        assert_eq!(String::from(Direction::from("eastbound")), "Eastbound");
        assert_eq!(String::from(Direction::from("Clockwise")), "Clockwise");
    }

    #[test]
    fn locale_conversions_agree() {
        assert_eq!(Locale::from("es"), Locale::new("es"));
        assert_eq!(Locale::from(String::from("es")), Locale("es".into()));
        assert_eq!(Locale::from("es").to_string(), "es");
    }

    #[test]
    fn ids_display_as_given() {
        assert_eq!(StopId(456).to_string(), "456");
        let id = "8A3D8B0D-D95E-4621-BAF8-E969867256B5";
        assert_eq!(DetourId::from(id).as_str(), id);
        assert_eq!(DetourId::from(id).to_string(), id);
        assert_eq!(DetourId::from(id), DetourId::new(String::from(id)));
    }

    #[test]
    fn dedup_keeps_first_and_join_uses_commas() {
        assert_eq!(dedup([3, 1, 3, 2, 1]), [3, 1, 2]);
        assert_eq!(join(&[StopId(456), StopId(4727)]), "456,4727");
        assert_eq!(join::<StopId>(&[]), "");
    }

    #[test]
    fn append_locale_only_when_set() {
        let mut url = Url::parse("https://example.com/x?key=k").unwrap();
        append_locale(&mut url, None);
        assert_eq!(url.query(), Some("key=k"));
        append_locale(&mut url, Some(&Locale::from("es")));
        assert_eq!(url.query(), Some("key=k&locale=es"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn direction_serializes_as_plain_string() {
        assert_eq!(
            serde_json::to_string(&Direction::Eastbound).unwrap(),
            r#""Eastbound""#
        );
        assert_eq!(
            serde_json::from_str::<Direction>(r#""EASTBOUND""#).unwrap(),
            Direction::Eastbound
        );
        assert_eq!(
            serde_json::from_str::<Direction>(r#""Clockwise""#).unwrap(),
            Direction::Other("Clockwise".into())
        );
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
