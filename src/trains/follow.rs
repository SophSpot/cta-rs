use super::arrivals::{RawEta, to_arrival};
use super::{Arrival, Position, Request, parse_envelope, position};
use crate::chicago;
use crate::deserializers::{one_or_many, opt_from_str};
use crate::{DecodeError, ResponseError};
use serde::Deserialize;
use url::Url;

/// One train's position and its predicted arrival at each upcoming station.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct FollowedTrain {
    /// The train's last reported position, if known.
    pub position: Option<Position>,
    /// The upcoming stops, in order. Each stop's own `position` is `None`.
    pub stops: Vec<Arrival>,
}

#[derive(Deserialize)]
struct RawFollow {
    #[serde(default)]
    position: Option<RawPosition>,
    #[serde(default, deserialize_with = "one_or_many")]
    eta: Vec<RawEta>,
}

#[derive(Deserialize)]
struct RawPosition {
    #[serde(default, deserialize_with = "opt_from_str")]
    lat: Option<f64>,
    #[serde(default, deserialize_with = "opt_from_str")]
    lon: Option<f64>,
    #[serde(default, deserialize_with = "opt_from_str")]
    heading: Option<u16>,
}

/// Build the request to follow the train with run number `run`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn follow_request(base: &Url, key: &str, run: &str) -> Request {
    let mut url = base
        .join("ttfollow.aspx")
        .expect("base URL must be http(s)");
    url.query_pairs_mut()
        .append_pair("key", key)
        .append_pair("runnumber", run)
        .append_pair("outputType", "JSON");
    Request { url }
}

/// Parse the body of a follow response.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error code, including
/// [`ApiErrorKind::RunNotFound`](crate::ApiErrorKind::RunNotFound) when the run isn't in
/// service, or [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_follow(body: &[u8]) -> Result<FollowedTrain, ResponseError> {
    let raw: RawFollow = parse_envelope(body)?;
    let tz = chicago()?;

    let stops = raw
        .eta
        .into_iter()
        .map(|e| to_arrival(e, &tz))
        .collect::<Result<_, DecodeError>>()?;

    Ok(FollowedTrain {
        position: raw.position.and_then(|p| position(p.lat, p.lon, p.heading)),
        stops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;
    use crate::trains::{DEFAULT_BASE_URL, Route, StationId};

    #[test]
    fn parses_fixture() {
        let train = parse_follow(include_bytes!("../../tests/fixtures/follow_125.json")).unwrap();

        let p = train.position.unwrap();
        assert_eq!((p.lat, p.lon, p.heading), (41.90479, -87.66932, 293));

        assert_eq!(train.stops.len(), 3);
        let first = &train.stops[0];
        assert_eq!(first.route, Route::Blue); // from "Blue Line"
        assert_eq!(first.station_id, StationId(40590));
        assert_eq!(first.run, "125");
        assert!(first.approaching);
        assert_eq!(train.stops[2].station, "O'Hare");
        // Position belongs to the train, not to each stop.
        assert!(train.stops.iter().all(|s| s.position.is_none()));
    }

    #[test]
    fn unknown_run_is_run_not_found() {
        let body = include_bytes!("../../tests/fixtures/follow_not_found.json");
        match parse_follow(body) {
            Err(ResponseError::Api(e)) => assert_eq!(e.kind(), ApiErrorKind::RunNotFound),
            other => panic!("expected RunNotFound, got {other:?}"),
        }
    }

    #[test]
    fn request_uses_runnumber() {
        let base = Url::parse(DEFAULT_BASE_URL).unwrap();
        let req = follow_request(&base, "k", "125");
        assert_eq!(req.url.path(), "/api/1.0/ttfollow.aspx");
        assert_eq!(req.url.query(), Some("key=k&runnumber=125&outputType=JSON"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn followed_train_round_trips_through_json() {
        let train = parse_follow(include_bytes!("../../tests/fixtures/follow_125.json")).unwrap();
        let json = serde_json::to_string(&train).unwrap();
        let back: FollowedTrain = serde_json::from_str(&json).unwrap();
        assert_eq!(train, back);
    }
}
