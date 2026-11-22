//! A client for the [CTA Train Tracker API](https://www.transitchicago.com/developers/traintracker/):
//! live arrival predictions, train locations, and individual train runs for the
//! Chicago 'L'.
//!
//! You need a free API key from the CTA to make requests.
//!
//! # Quick start (async)
//!
//! With the default `reqwest` feature:
//!
//! ```no_run
//! # #[cfg(feature = "reqwest")]
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use cta::AsyncClient;
//! use cta::trains::{ArrivalsQuery, StationId};
//!
//! let client = AsyncClient::builder().train_key("your-api-key").build();
//!
//! // Clark/Lake
//! let arrivals = client.trains().arrivals(&ArrivalsQuery::at(StationId(40380))).await?;
//! for a in &arrivals {
//!     println!("{} line to {} in {}", a.route, a.destination, a.eta());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Quick start (blocking)
//!
//! With the `ureq` feature:
//!
//! ```no_run
//! # #[cfg(feature = "ureq")]
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use cta::BlockingClient;
//! use cta::trains::{LocationsQuery, Route};
//!
//! let client = BlockingClient::builder().train_key("your-api-key").build();
//!
//! for train in client.trains().locations(&LocationsQuery::route(Route::Red))? {
//!     println!("run {} next stop {}", train.run, train.next_station);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Endpoints
//!
//! | Method | What it returns |
//! |---|---|
//! | `arrivals` | Predicted arrivals at up to 4 stations or platforms ([`trains::Arrival`]) |
//! | `locations` | Every train currently running on one or more lines ([`trains::TrainLocation`]) |
//! | `follow` | One train's position and its upcoming stops ([`trains::FollowedTrain`]) |
//!
//! These are on [`AsyncClient::trains`] and [`BlockingClient::trains`].
//!
//! # Keys
//!
//! Each CTA API has its own key, set on the [`ClientBuilder`](client::ClientBuilder). A
//! client without a key is valid: calling an API whose key isn't set returns
//! [`Error::MissingKey`] without sending a request.
//!
//! # Features
//!
//! - `reqwest` (default): the default HTTP client for [`AsyncClient`], backed by [`reqwest`](https://docs.rs/reqwest).
//! - `ureq`: the default HTTP client for [`BlockingClient`], backed by [`ureq`](https://docs.rs/ureq).
//! - `serde`: `Serialize`/`Deserialize` for the response types, for caching or forwarding them.
//!
//! Both HTTP clients use rustls for TLS. To use a different HTTP client, implement
//! [`AsyncHttpClient`] or [`BlockingHttpClient`] and pass it to
//! [`ClientBuilder::http`](client::ClientBuilder::http).
//!
//! # Without an HTTP client
//!
//! The request builders and parsers in [`trains`] (such as [`trains::arrivals_request`] and
//! [`trains::parse_arrivals`]) do no I/O, so you can send the request however you like and
//! hand the response body to the parser.
//!
//! # Times
//!
//! The CTA reports Chicago local time without an offset. This crate attaches the
//! `America/Chicago` time zone, so every time is a [`jiff::ZonedDateTime`]. `jiff` is
//! re-exported so you can use it without adding a matching dependency.

#![warn(missing_docs)]

pub mod buses;
pub mod client;
mod deserializers;
pub mod trains;
pub use client::{AsyncClient, AsyncHttpClient, BlockingClient, BlockingHttpClient};
/// Re-export of [`jiff`], the date and time library used in this crate's public types.
pub use jiff;
use jiff::tz::TimeZone;
use std::fmt::Formatter;
/// Re-export of [`url::Url`], used by [`AsyncHttpClient`], [`BlockingHttpClient`], and the builder's base URLs.
pub use url::Url;

/// A CTA API: which key is missing, or which API reported an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Api {
    /// The Train Tracker API: arrivals, locations, and follow.
    Train,
    /// The Bus Tracker API: predictions, vehicles, routes, stops, patterns, and detours.
    Bus,
}

impl std::fmt::Display for Api {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Api::Bus => "Bus Tracker",
            Api::Train => "Train Tracker",
        })
    }
}

/// An error reported by the CTA API itself.
///
/// The request reached the CTA and got a well-formed response, but the response
/// reported an error (for example, an invalid key or an unknown station).
/// Use [`kind`](ApiError::kind) to match on known errors.
///
/// Train Tracker errors have a numeric [`code`](ApiError::code). Bus Tracker errors
/// only have a message, so their `code` is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ApiError {
    /// The API that reported the error.
    pub api: Api,
    /// The CTA's numeric error code (`errCd`). Always `None` for Bus Tracker errors.
    pub code: Option<u16>,
    /// The CTA's error message, e.g. `"Invalid API key"`.
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Some(code) => write!(f, "{} error {code}: {}", self.api, self.message),
            None => write!(f, "{} error: {}", self.api, self.message),
        }
    }
}

impl std::error::Error for ApiError {}

/// Known CTA errors, as returned by [`ApiError::kind`].
///
/// Train Tracker errors are classified by code, Bus Tracker errors by message. Kinds
/// marked "Train Tracker only" are never returned for Bus Tracker errors.
///
/// New kinds may be added in future versions, so a `match` needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiErrorKind {
    /// The daily request limit for this key was reached.
    ///
    /// Train Tracker code 102; Bus Tracker "Transaction limit for current day has been exceeded."
    DailyLimitExceeded,
    /// The API key is not valid.
    ///
    /// Train Tracker code 101; Bus Tracker "Invalid API access key supplied".
    InvalidKey,
    /// The station ID (`mapid`) is not valid (103). Train Tracker only.
    InvalidStation,
    /// The route is not valid (106). Train Tracker only.
    InvalidRoute,
    /// The stop ID (`stpid`) is not valid (108). Train Tracker only.
    InvalidStop,
    /// The `max` value is not valid (110). Train Tracker only.
    InvalidMax,
    /// A required parameter was missing from the request (100). Train Tracker only.
    ///
    /// This is the CTA's error. The client-side error for a key that was never
    /// configured is [`Error::MissingKey`].
    MissingParameter,
    /// No train with this run number is in service (501). Train Tracker only.
    ///
    /// Expected in normal use: a run ends when the train reaches its terminal.
    RunNotFound,
    /// The train exists, but the CTA can't determine its upcoming stops (502). Train Tracker only.
    ///
    /// Returned by `follow`. The train may be heading to an unexpected terminal.
    NoUpcomingStops,
    /// The train exists, but none of its predictions are for active stations (503). Train
    /// Tracker only.
    ///
    /// Returned by `follow`.
    NoPredictions,
    /// The CTA had an internal error.
    ///
    /// Train Tracker code 900; Bus Tracker "Internal server error - Unable to complete
    /// request at this time".
    ServerError,
    /// An error without a named kind. Check [`ApiError::code`] and [`ApiError::message`].
    Other,
}

impl ApiError {
    /// A Train Tracker error with its `errCd` and `errNm`.
    pub(crate) fn train(code: u16, message: String) -> Self {
        Self {
            api: Api::Train,
            code: Some(code),
            message,
        }
    }

    /// A Bus Tracker error. Bus Tracker errors have no code.
    pub(crate) fn bus(message: String) -> Self {
        Self {
            api: Api::Bus,
            code: None,
            message,
        }
    }

    /// Classify the error.
    ///
    /// ```
    /// use cta::{ApiErrorKind, ResponseError};
    ///
    /// let body = br#"{"ctatt":{"errCd":"101","errNm":"Invalid API key"}}"#;
    /// match cta::trains::parse_arrivals(body) {
    ///     Err(ResponseError::Api(e)) => assert_eq!(e.kind(), ApiErrorKind::InvalidKey),
    ///     other => panic!("expected an API error, got {other:?}"),
    /// }
    /// ```
    pub fn kind(&self) -> ApiErrorKind {
        match (self.api, self.code) {
            (Api::Train, Some(code)) => train_kind(code),
            (Api::Train, None) => ApiErrorKind::Other,
            (Api::Bus, _) => bus_kind(&self.message),
        }
    }
}

fn train_kind(code: u16) -> ApiErrorKind {
    match code {
        100 => ApiErrorKind::MissingParameter,
        101 => ApiErrorKind::InvalidKey,
        102 => ApiErrorKind::DailyLimitExceeded,
        103 => ApiErrorKind::InvalidStation,
        106 => ApiErrorKind::InvalidRoute,
        108 => ApiErrorKind::InvalidStop,
        110 => ApiErrorKind::InvalidMax,
        501 => ApiErrorKind::RunNotFound,
        502 => ApiErrorKind::NoUpcomingStops,
        503 => ApiErrorKind::NoPredictions,
        900 => ApiErrorKind::ServerError,
        _ => ApiErrorKind::Other,
    }
}

/// Bus Tracker errors have no code, only the message text listed in its documentation.
fn bus_kind(message: &str) -> ApiErrorKind {
    let message = message.trim();
    if message == "Invalid API access key supplied" {
        ApiErrorKind::InvalidKey
    } else if message.starts_with("Transaction limit for current day has been exceeded") {
        ApiErrorKind::DailyLimitExceeded
    } else if message.starts_with("Internal server error") {
        ApiErrorKind::ServerError
    } else {
        ApiErrorKind::Other
    }
}
/// The response couldn't be read.
///
/// The body wasn't valid JSON, didn't have the expected shape, or contained a value this
/// crate doesn't understand (such as an unknown route or a malformed time). The
/// underlying cause is available through [`std::error::Error::source`].
#[derive(Debug, thiserror::Error)]
#[error("could not read CTA response")]
pub struct DecodeError(#[source] Box<dyn std::error::Error + Send + Sync>);

impl From<serde_json::Error> for DecodeError {
    fn from(e: serde_json::Error) -> Self {
        Self(Box::new(e))
    }
}

impl From<trains::ParseRouteError> for DecodeError {
    fn from(e: trains::ParseRouteError) -> Self {
        Self(Box::new(e))
    }
}

impl From<jiff::Error> for DecodeError {
    fn from(e: jiff::Error) -> Self {
        Self(Box::new(e))
    }
}

/// The error returned by the response parsers in [`trains`], such as
/// [`trains::parse_arrivals`].
///
/// It converts into [`Error`] with `?`.
#[derive(Debug, thiserror::Error)]
pub enum ResponseError {
    /// The CTA returned an error code.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// The response couldn't be read.
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

/// The error returned by [`AsyncClient`] and [`BlockingClient`] methods.
///
/// `E` is the HTTP client's error type, e.g. `reqwest::Error`.
///
/// ```no_run
/// # #[cfg(feature = "reqwest")]
/// # async fn run(client: cta::AsyncClient<reqwest::Client>) {
/// use cta::{ApiErrorKind, Error};
///
/// match client.trains().follow("123").await {
///     Ok(train) => println!("{} stops left", train.stops.len()),
///     Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound => println!("run ended"),
///     Err(e) => eprintln!("{e}"),
/// }
/// # }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error<E> {
    /// The client has no key for the API this method calls.
    ///
    /// No request was sent. Set the key on the builder with
    /// [`ClientBuilder::train_key`](client::ClientBuilder::train_key) or
    /// [`ClientBuilder::bus_key`](client::ClientBuilder::bus_key).
    #[error("no API key set for {api}")]
    MissingKey {
        /// The API whose key is missing.
        api: Api,
    },
    /// The HTTP request failed: connection error, timeout, or a non-success status.
    #[error("HTTP request failed")]
    Http(#[source] E),
    /// The CTA returned an error code.
    #[error(transparent)]
    Api(#[from] ApiError),
    /// The response couldn't be read.
    #[error(transparent)]
    Decode(#[from] DecodeError),
}

impl<E> From<ResponseError> for Error<E> {
    fn from(e: ResponseError) -> Self {
        match e {
            ResponseError::Api(e) => Error::Api(e),
            ResponseError::Decode(e) => Error::Decode(e),
        }
    }
}

/// A request to send to the CTA, built by functions like [`trains::arrivals_request`].
///
/// The `Debug` output hides the API key.
#[derive(Clone, PartialEq)]
pub struct Request {
    /// The full URL to GET, including the API key.
    pub url: Url,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut url = self.url.clone();
        let pairs: Vec<(String, String)> = self
            .url
            .query_pairs()
            .map(|(k, v)| {
                let v = if k == "key" {
                    "REDACTED".into()
                } else {
                    v.into_owned()
                };
                (k.into_owned(), v)
            })
            .collect();
        url.query_pairs_mut().clear().extend_pairs(pairs);
        f.debug_struct("Request")
            .field("url", &url.as_str())
            .finish()
    }
}

/// A point on the map, in decimal degrees (WGS 84).
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Coordinates {
    /// Latitude in degrees, e.g. `41.88`.
    pub lat: f64,
    /// Longitude in degrees, e.g. `-87.64`.
    pub lon: f64,
}

pub(crate) fn chicago() -> Result<TimeZone, DecodeError> {
    Ok(TimeZone::get("America/Chicago")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(code: u16) -> ApiError {
        ApiError::train(code, String::new())
    }

    /// The CTA code each named kind comes from. The exhaustive `match`
    /// means adding a variant to `ApiErrorKind` won't compile until it's listed here.
    fn code_for(kind: ApiErrorKind) -> Option<u16> {
        match kind {
            ApiErrorKind::MissingParameter => Some(100),
            ApiErrorKind::InvalidKey => Some(101),
            ApiErrorKind::DailyLimitExceeded => Some(102),
            ApiErrorKind::InvalidStation => Some(103),
            ApiErrorKind::InvalidRoute => Some(106),
            ApiErrorKind::InvalidStop => Some(108),
            ApiErrorKind::InvalidMax => Some(110),
            ApiErrorKind::RunNotFound => Some(501),
            ApiErrorKind::NoUpcomingStops => Some(502),
            ApiErrorKind::NoPredictions => Some(503),
            ApiErrorKind::ServerError => Some(900),
            ApiErrorKind::Other => None,
        }
    }

    const NAMED: [ApiErrorKind; 11] = [
        ApiErrorKind::MissingParameter,
        ApiErrorKind::InvalidKey,
        ApiErrorKind::DailyLimitExceeded,
        ApiErrorKind::InvalidStation,
        ApiErrorKind::InvalidRoute,
        ApiErrorKind::InvalidStop,
        ApiErrorKind::InvalidMax,
        ApiErrorKind::RunNotFound,
        ApiErrorKind::NoPredictions,
        ApiErrorKind::NoUpcomingStops,
        ApiErrorKind::ServerError,
    ];

    #[test]
    fn every_named_kind_maps_from_its_code() {
        for kind in NAMED {
            let code = code_for(kind).expect("named kinds have a code");
            assert_eq!(api(code).kind(), kind, "code {code}");
        }
    }

    #[test]
    fn codes_are_unique() {
        let mut codes: Vec<u16> = NAMED.iter().filter_map(|k| code_for(*k)).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), NAMED.len());
    }

    #[test]
    fn unlisted_codes_are_other() {
        for code in [1, 104, 105, 107, 109, 111, 500, 504, 999, u16::MAX] {
            assert_eq!(api(code).kind(), ApiErrorKind::Other, "code {code}");
        }
    }

    #[test]
    fn code_is_kept_for_other() {
        // Nothing is lost when a code has no named kind.
        let e = ApiError::train(104, "Something new".into());
        assert_eq!(e.kind(), ApiErrorKind::Other);
        assert_eq!(e.code, Some(104));
        assert_eq!(e.to_string(), "Train Tracker error 104: Something new");
    }

    #[test]
    fn display_includes_code_and_message() {
        let e = ApiError::train(101, "Invalid API key".into());
        assert_eq!(e.to_string(), "Train Tracker error 101: Invalid API key");
    }

    #[test]
    fn bus_messages_map_to_kinds() {
        for (message, kind) in [
            ("Invalid API access key supplied", ApiErrorKind::InvalidKey),
            (
                "Transaction limit for current day has been exceeded.",
                ApiErrorKind::DailyLimitExceeded,
            ),
            (
                "Internal server error - Unable to complete request at this time",
                ApiErrorKind::ServerError,
            ),
            ("No data found for parameter", ApiErrorKind::Other),
            ("Something new", ApiErrorKind::Other),
        ] {
            assert_eq!(ApiError::bus(message.into()).kind(), kind, "{message}");
        }
    }

    #[test]
    fn bus_codes_are_ignored() {
        // A Bus Tracker error is never classified by a train code.
        let e = ApiError {
            api: Api::Bus,
            code: Some(101),
            message: "Something new".into(),
        };
        assert_eq!(e.kind(), ApiErrorKind::Other);
    }

    #[test]
    fn bus_display_has_no_code() {
        let e = ApiError::bus("Invalid API access key supplied".into());
        assert_eq!(e.code, None);
        assert_eq!(
            e.to_string(),
            "Bus Tracker error: Invalid API access key supplied"
        );
    }
}
