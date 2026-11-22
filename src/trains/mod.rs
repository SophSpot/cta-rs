//! The Train Tracker API: arrivals, train locations, and following a train.
//!
//! Most users only need the query and response types here and call the methods on
//! [`AsyncClient`](crate::AsyncClient) or [`BlockingClient`](crate::BlockingClient).
//! The `*_request` and `parse_*` functions are for sending requests yourself: they
//! build the URL and parse the response body without doing any I/O.
//!
//! ```
//! use cta::{Request, Url};
//! use cta::trains::{ArrivalsQuery, DEFAULT_BASE_URL, StationId, arrivals_request, parse_arrivals};
//!
//! let base = Url::parse(DEFAULT_BASE_URL)?;
//! let request = arrivals_request(&base, "your-api-key", &ArrivalsQuery::at(StationId(40380)));
//! // ...send a GET to request.url with any HTTP client, then:
//! # let body = br#"{"ctatt":{"errCd":"0","errNm":null}}"#;
//! let arrivals = parse_arrivals(body)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod arrivals;
mod follow;
mod locations;

use crate::deserializers::from_str;
use crate::{ApiError, DecodeError, ResponseError};
pub use arrivals::{
    Arrival, ArrivalsQuery, Location, QueryError, arrivals_request, parse_arrivals,
};
pub use follow::{FollowedTrain, follow_request, parse_follow};
pub use locations::{LocationsQuery, TrainLocation, locations_request, parse_locations};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// The Train Tracker API's base URL. The clients use this unless given another with `base_url`.
pub const DEFAULT_BASE_URL: &str = "https://lapi.transitchicago.com/api/1.0/";

/// A CTA 'L' line.
///
/// Parses from the CTA's route codes (`"Brn"`), line names (`"Brown"`), and
/// `"Brown Line"`, ignoring case. Displays as the line name.
///
/// ```
/// use cta::trains::Route;
///
/// let route: Route = "Brn".parse()?;
/// assert_eq!(route, Route::Brown);
/// assert_eq!(route.to_string(), "Brown");
/// assert_eq!(route.code(), "Brn");
/// # Ok::<(), cta::trains::ParseRouteError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Route {
    /// Blue Line (O'Hare – Forest Park).
    Blue,
    /// Brown Line (Kimball – Loop).
    Brown,
    /// Green Line (Harlem/Lake – Ashland/63rd, Cottage Grove).
    Green,
    /// Red Line (Howard – 95th/Dan Ryan).
    Red,
    /// Orange Line (Midway – Loop).
    Orange,
    /// Pink Line (54th/Cermak – Loop).
    Pink,
    /// Purple Line (Linden – Howard, Loop at rush hour).
    Purple,
    /// Yellow Line (Skokie – Howard).
    Yellow,
}

/// A station ID (`mapid`, 4xxxx), covering all platforms at a station.
///
/// Station IDs are listed in the CTA's
/// [stops data](https://data.cityofchicago.org/Transportation/CTA-System-Information-List-of-L-Stops/8pix-ypme).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StationId(pub u32);

impl Display for StationId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A stop ID (`stpid`, 3xxxx): one platform, usually one direction, at a station.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StopId(pub u32);

impl Display for StopId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Route {
    /// Every line.
    pub const ALL: [Route; 8] = [
        Route::Red,
        Route::Blue,
        Route::Brown,
        Route::Green,
        Route::Orange,
        Route::Purple,
        Route::Pink,
        Route::Yellow,
    ];

    /// The code the CTA API uses for this line, e.g. `"Brn"` for [`Route::Brown`].
    pub const fn code(self) -> &'static str {
        match self {
            Route::Red => "Red",
            Route::Blue => "Blue",
            Route::Brown => "Brn",
            Route::Green => "G",
            Route::Orange => "Org",
            Route::Purple => "P",
            Route::Pink => "Pink",
            Route::Yellow => "Y",
        }
    }

    /// The line's name, e.g. `"Brown"`. The same as its `Display` output.
    pub const fn name(self) -> &'static str {
        match self {
            Route::Red => "Red",
            Route::Blue => "Blue",
            Route::Brown => "Brown",
            Route::Green => "Green",
            Route::Orange => "Orange",
            Route::Purple => "Purple",
            Route::Pink => "Pink",
            Route::Yellow => "Yellow",
        }
    }
}

/// The error from parsing a string that isn't a known [`Route`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown route: {0:?}")]
pub struct ParseRouteError(String);

impl FromStr for Route {
    type Err = ParseRouteError;

    /// Accepts API codes ("Brn"), names ("brown"), and "Brown Line", case-insensitive.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.trim().to_ascii_lowercase();
        let key = lower.strip_suffix(" line").unwrap_or(&lower);

        Route::ALL
            .into_iter()
            .find(|r| r.code().eq_ignore_ascii_case(key) || r.name().eq_ignore_ascii_case(key))
            .ok_or_else(|| ParseRouteError(s.to_owned()))
    }
}

impl Display for Route {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        // `pad` (not `write_str`) so `{:<8}` / `{:>8}` alignment works in tables
        f.pad(self.name())
    }
}

/// A train's last reported position.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Position {
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
    /// Compass heading in degrees, 0–359.
    pub heading: u16,
}

#[derive(Deserialize)]
struct RawEnvelope<T> {
    #[serde(rename = "errCd", deserialize_with = "from_str")]
    err_cd: u16,
    #[serde(rename = "errNm")]
    err_nm: Option<String>,
    #[serde(flatten)]
    body: T,
}

#[derive(Deserialize)]
struct RawResponse<T> {
    ctatt: RawEnvelope<T>,
}

pub(super) fn parse_envelope<T: DeserializeOwned>(body: &[u8]) -> Result<T, ResponseError> {
    let env = serde_json::from_slice::<RawResponse<T>>(body)
        .map_err(DecodeError::from)?
        .ctatt;
    if env.err_cd != 0 {
        return Err(ApiError::train(env.err_cd, env.err_nm.unwrap_or_default()).into());
    }
    Ok(env.body)
}

pub(super) fn position(
    lat: Option<f64>,
    lon: Option<f64>,
    heading: Option<u16>,
) -> Option<Position> {
    match (lat, lon, heading) {
        (Some(lat), Some(lon), Some(heading)) => Some(Position { lat, lon, heading }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiErrorKind, Request};

    #[test]
    fn route_parses_codes_names_and_line_suffix() {
        for r in Route::ALL {
            assert_eq!(r.code().parse::<Route>(), Ok(r));
            assert_eq!(r.name().parse::<Route>(), Ok(r));
            assert_eq!(r.to_string().parse::<Route>(), Ok(r));
        }
        // Forms seen in real CTA responses: arrivals, locations, follow.
        assert_eq!("Brn".parse::<Route>(), Ok(Route::Brown));
        assert_eq!("brn".parse::<Route>(), Ok(Route::Brown));
        assert_eq!("Blue Line".parse::<Route>(), Ok(Route::Blue));
        assert_eq!("y".parse::<Route>(), Ok(Route::Yellow));
    }

    #[test]
    fn unknown_route_is_error() {
        assert!("Silver".parse::<Route>().is_err());
        assert!("".parse::<Route>().is_err());
    }

    #[test]
    fn route_display_supports_padding() {
        assert_eq!(format!("[{:<6}]", Route::Red), "[Red   ]");
    }

    #[test]
    fn ids_display_as_numbers() {
        assert_eq!(StationId(40380).to_string(), "40380");
        assert_eq!(StopId(30074).to_string(), "30074");
    }

    #[test]
    fn position_needs_all_three_parts() {
        assert_eq!(
            position(Some(41.9), Some(-87.6), Some(90)),
            Some(Position {
                lat: 41.9,
                lon: -87.6,
                heading: 90
            })
        );
        assert_eq!(position(Some(41.9), None, Some(90)), None);
        assert_eq!(position(None, None, None), None);
    }

    #[test]
    fn request_debug_hides_key() {
        let url = url::Url::parse("https://example.com/x?key=secret&mapid=40380").unwrap();
        let out = format!("{:?}", Request { url });
        assert!(!out.contains("secret"), "{out}");
        assert!(out.contains("REDACTED"));
        assert!(out.contains("mapid=40380"));
    }

    #[derive(Debug, Deserialize)]
    struct Empty {}

    #[test]
    fn envelope_error_code_becomes_api_error() {
        let body = br#"{"ctatt":{"errCd":"101","errNm":"Invalid API key"}}"#;
        match parse_envelope::<Empty>(body) {
            Err(ResponseError::Api(e)) => {
                assert_eq!(e.api, crate::Api::Train);
                assert_eq!(e.code, Some(101));
                assert_eq!(e.kind(), ApiErrorKind::InvalidKey);
                assert_eq!(e.message, "Invalid API key");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[test]
    fn envelope_garbage_is_decode_error() {
        assert!(matches!(
            parse_envelope::<Empty>(b"not json"),
            Err(ResponseError::Decode(_))
        ));
        assert!(matches!(
            parse_envelope::<Empty>(b"{}"),
            Err(ResponseError::Decode(_))
        ));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn route_serializes_as_readable_name() {
        assert_eq!(serde_json::to_string(&Route::Brown).unwrap(), r#""Brown""#);
        assert_eq!(
            serde_json::from_str::<Route>(r#""Brown""#).unwrap(),
            Route::Brown
        );
    }
}
