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
//! let client = AsyncClient::new("your-api-key");
//!
//! // Clark/Lake
//! let arrivals = client.arrivals(&ArrivalsQuery::at(StationId(40380))).await?;
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
//! let client = BlockingClient::new("your-api-key");
//!
//! for train in client.locations(&LocationsQuery::route(Route::Red))? {
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
//! | `arrivals` | Predicted arrivals at a station or a single platform ([`trains::Arrival`]) |
//! | `locations` | Every train currently running on one or more lines ([`trains::TrainLocation`]) |
//! | `follow` | One train's position and its upcoming stops ([`trains::FollowedTrain`]) |
//!
//! Each method exists on both [`AsyncClient`] and [`BlockingClient`].
//!
//! # Features
//!
//! - `reqwest` (default): [`AsyncClient::new`], backed by [`reqwest`](https://docs.rs/reqwest).
//! - `ureq`: [`BlockingClient::new`], backed by [`ureq`](https://docs.rs/ureq).
//! - `serde`: `Serialize`/`Deserialize` for the response types, for caching or forwarding them.
//!
//! Both HTTP clients use rustls for TLS. To use a different HTTP client, implement
//! [`AsyncHttpClient`] or [`BlockingHttpClient`] and pass it to `with_http`.
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

pub mod client;
mod deserializers;
pub mod trains;
pub use client::{AsyncClient, AsyncHttpClient, BlockingClient, BlockingHttpClient};
/// Re-export of [`jiff`], the date and time library used in this crate's public types.
pub use jiff;
use jiff::tz::TimeZone;
/// Re-export of [`url::Url`], used by [`AsyncHttpClient`], [`BlockingHttpClient`], and `base_url`.
pub use url::Url;

/// An error reported by the CTA API itself.
///
/// The request reached the CTA and got a well-formed response, but the response
/// carried a non-zero error code (for example, an invalid key or an unknown station).
/// Use [`kind`](ApiError::kind) to match on known errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("CTA API error {code}: {message}")]
pub struct ApiError {
    /// The CTA's numeric error code (`errCd`).
    pub code: u16,
    /// The CTA's error message (`errNm`), e.g. `"Invalid API key"`.
    pub message: String,
}

/// Known CTA error codes, as returned by [`ApiError::kind`].
///
/// New kinds may be added in future versions, so a `match` needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiErrorKind {
    /// The daily request limit for this key was reached (102).
    DailyLimitExceeded,
    /// The API key is not valid (101).
    InvalidKey,
    /// The station ID (`mapid`) is not valid (103).
    InvalidStation,
    /// The route is not valid (106).
    InvalidRoute,
    /// The stop ID (`stpid`) is not valid (108).
    InvalidStop,
    /// The `max` value is not valid (110).
    InvalidMax,
    /// A required parameter was missing from the request (100).
    MissingParameter,
    /// No train with this run number is in service (501).
    ///
    /// Expected in normal use: a run ends when the train reaches its terminal.
    RunNotFound,
    /// The train exists, but the CTA can't determine its upcoming stops (502).
    ///
    /// Returned by `follow`. The train may be heading to an unexpected terminal.
    NoUpcomingStops,
    /// The train exists, but none of its predictions are for active stations (503).
    ///
    /// Returned by `follow`.
    NoPredictions,
    /// The CTA had an internal error (900).
    ServerError,
    /// A code without a named kind. Check [`ApiError::code`].
    Other,
}

impl ApiError {
    /// Classify the error code.
    ///
    /// ```
    /// use cta::{ApiError, ApiErrorKind};
    ///
    /// let err = ApiError { code: 101, message: "Invalid API key".into() };
    /// assert_eq!(err.kind(), ApiErrorKind::InvalidKey);
    /// ```
    pub fn kind(&self) -> ApiErrorKind {
        match self.code {
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
/// match client.follow("123").await {
///     Ok(train) => println!("{} stops left", train.stops.len()),
///     Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound => println!("run ended"),
///     Err(e) => eprintln!("{e}"),
/// }
/// # }
/// ```
#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
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

pub(crate) fn chicago() -> Result<TimeZone, DecodeError> {
    Ok(TimeZone::get("America/Chicago")?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(code: u16) -> ApiError {
        ApiError {
            code,
            message: String::new(),
        }
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
        let e = ApiError {
            code: 104,
            message: "Something new".into(),
        };
        assert_eq!(e.kind(), ApiErrorKind::Other);
        assert_eq!(e.code, 104);
        assert_eq!(e.to_string(), "CTA API error 104: Something new");
    }

    #[test]
    fn display_includes_code_and_message() {
        let e = ApiError {
            code: 101,
            message: "Invalid API key".into(),
        };
        assert_eq!(e.to_string(), "CTA API error 101: Invalid API key");
    }
}
