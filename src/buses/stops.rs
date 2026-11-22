use super::{
    DetourId, Direction, Locale, QueryError, Route, StopId, append_locale, dedup, join,
    parse_envelope,
};
use crate::deserializers::{num_or_str, one_or_many, opt_bool_or_str, opt_num_or_str};
use crate::{Coordinates, Request, ResponseError};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawStops {
    #[serde(default, deserialize_with = "one_or_many")]
    stops: Vec<RawStop>,
}

#[derive(Deserialize)]
struct RawStop {
    #[serde(deserialize_with = "num_or_str")]
    stpid: u32,
    stpnm: String,
    #[serde(deserialize_with = "num_or_str")]
    lat: f64,
    #[serde(deserialize_with = "num_or_str")]
    lon: f64,
    #[serde(default, deserialize_with = "one_or_many")]
    dtradd: Vec<String>,
    #[serde(default, deserialize_with = "one_or_many")]
    dtrrem: Vec<String>,
    #[serde(default, deserialize_with = "opt_bool_or_str")]
    ada: Option<bool>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    gtfsseq: Option<u32>,
}

/// A bus stop, as returned by [`Buses::stops`](crate::client::Buses::stops).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Stop {
    /// The stop's ID, e.g. `456`.
    pub id: StopId,
    /// The stop's name, e.g. `"Madison & Jefferson"`, without the stray spaces Bus
    /// Tracker sometimes adds.
    ///
    /// Bus Tracker can translate this with [`StopsQuery::locale`], but CTA returns
    /// English in every locale today.
    pub name: String,
    /// Where the stop is.
    pub location: Coordinates,
    /// Detours that temporarily serve this stop. Usually empty.
    pub detours_added: Vec<DetourId>,
    /// Detours that temporarily skip this stop. Usually empty.
    pub detours_removed: Vec<DetourId>,
    /// Whether the stop is accessible under the ADA, if CTA says.
    ///
    /// Bus Tracker documents this field, but CTA doesn't send it today, so it is
    /// always `None`.
    pub accessible: Option<bool>,
    /// The stop's position in CTA's GTFS schedule data, if CTA says.
    ///
    /// Bus Tracker documents this field, but CTA doesn't send it today, so it is
    /// always `None`.
    pub gtfs_sequence: Option<u32>,
}

/// Which stops a stops query covers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StopsTarget {
    /// Every stop on a route in one direction.
    Route { route: Route, direction: Direction },
    /// Up to 10 stops by ID. Never empty.
    Stops(Vec<StopId>),
}

/// Which stops to fetch: every stop on a route in one direction, or stops by ID.
///
/// ```
/// use cta::buses::{Direction, StopId, StopsQuery};
///
/// // Every eastbound stop on route 20 (Madison)
/// let route = StopsQuery::route("20", Direction::Eastbound);
///
/// // One stop, or up to 10
/// let one = StopsQuery::at(StopId(456));
/// let some = StopsQuery::stops([StopId(456), StopId(4727)])?;
/// # Ok::<(), cta::buses::QueryError>(())
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopsQuery {
    target: StopsTarget,
    locale: Option<Locale>,
}

impl StopsQuery {
    /// The most stop IDs one query can cover.
    pub const MAX_STOPS: usize = 10;

    fn new(target: StopsTarget) -> Self {
        Self {
            target,
            locale: None,
        }
    }

    /// Every stop on `route` going in `direction`.
    ///
    /// The stops come in no particular order.
    pub fn route(route: impl Into<Route>, direction: impl Into<Direction>) -> Self {
        Self::new(StopsTarget::Route {
            route: route.into(),
            direction: direction.into(),
        })
    }

    /// One stop by ID.
    pub fn at(stop: StopId) -> Self {
        Self::new(StopsTarget::Stops(vec![stop]))
    }

    /// Up to 10 stops by ID in one request. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoStops`] if `ids` is empty, or [`QueryError::TooManyStops`] if it
    /// has more than 10 distinct IDs.
    pub fn stops(ids: impl IntoIterator<Item = StopId>) -> Result<Self, QueryError> {
        let ids = dedup(ids);
        match ids.len() {
            0 => Err(QueryError::NoStops),
            n if n > Self::MAX_STOPS => Err(QueryError::TooManyStops(n)),
            _ => Ok(Self::new(StopsTarget::Stops(ids))),
        }
    }

    /// The language for stop names. See [`Locale`] for what CTA translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for the stops in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn stops_request(base: &Url, key: &str, query: &StopsQuery) -> Request {
    let mut url = base.join("getstops").expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("key", key);
        match &query.target {
            StopsTarget::Route { route, direction } => {
                q.append_pair("rt", route.as_str())
                    .append_pair("dir", direction.as_str());
            }
            StopsTarget::Stops(ids) => {
                q.append_pair("stpid", &join(ids));
            }
        }
    }
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a stops response.
///
/// Stops that don't exist or have no service are left out, not reported as errors:
/// Bus Tracker reports both the same way.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_stops(body: &[u8]) -> Result<Vec<Stop>, ResponseError> {
    let raw: RawStops = parse_envelope(body)?;
    Ok(raw
        .stops
        .into_iter()
        .map(|s| Stop {
            id: StopId(s.stpid),
            name: s.stpnm.trim().to_owned(),
            location: Coordinates {
                lat: s.lat,
                lon: s.lon,
            },
            detours_added: s.dtradd.into_iter().map(DetourId).collect(),
            detours_removed: s.dtrrem.into_iter().map(DetourId).collect(),
            accessible: s.ada,
            gtfs_sequence: s.gtfsseq,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const MADISON_EAST: &[u8] = include_bytes!("../../tests/fixtures/bus_stops_20_eastbound.json");
    const BY_ID: &[u8] = include_bytes!("../../tests/fixtures/bus_stops_by_id.json");
    const DETOURS: &[u8] = include_bytes!("../../tests/fixtures/bus_stops_with_detours.json");
    const TOO_MANY: &[u8] = include_bytes!("../../tests/fixtures/bus_stops_too_many.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    const DETOUR: &str = "8A3D8B0D-D95E-4621-BAF8-E969867256B5";

    fn query_string(query: &StopsQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = stops_request(&base, "k", query).url;
        assert_eq!(url.path(), "/bustime/api/v3/getstops");
        url.query().unwrap().to_owned()
    }

    fn stop(stops: &[Stop], id: u32) -> &Stop {
        stops
            .iter()
            .find(|s| s.id == StopId(id))
            .unwrap_or_else(|| panic!("no stop {id}"))
    }

    #[test]
    fn request_for_route_and_direction() {
        assert_eq!(
            query_string(&StopsQuery::route("20", Direction::Eastbound)),
            "key=k&rt=20&dir=Eastbound&format=json"
        );
        // Text is converted to a direction, and the canonical form is sent.
        assert_eq!(
            query_string(&StopsQuery::route("X9", "NORTHBOUND").locale("es")),
            "key=k&rt=X9&dir=Northbound&locale=es&format=json"
        );
    }

    #[test]
    fn request_for_stop_ids() {
        assert_eq!(
            query_string(&StopsQuery::at(StopId(456))),
            "key=k&stpid=456&format=json"
        );
        let q = StopsQuery::stops([StopId(456), StopId(4727), StopId(456)]).unwrap();
        assert_eq!(query_string(&q), "key=k&stpid=456%2C4727&format=json");
    }

    #[test]
    fn stop_id_limits() {
        assert_eq!(StopsQuery::stops([]), Err(QueryError::NoStops));
        let ten = (1..=10).map(StopId);
        assert!(StopsQuery::stops(ten).is_ok());
        let eleven = (1..=11).map(StopId);
        assert_eq!(StopsQuery::stops(eleven), Err(QueryError::TooManyStops(11)));
        // Duplicates don't count toward the limit.
        let ten_with_repeats = (1..=10).chain(1..=5).map(StopId);
        assert!(StopsQuery::stops(ten_with_repeats).is_ok());
    }

    #[test]
    fn parses_route_and_trims_names() {
        let stops = parse_stops(MADISON_EAST).unwrap();
        assert_eq!(stops.len(), 66);
        let first = &stops[0];
        assert_eq!(first.id, StopId(4727));
        assert_eq!(first.name, "1633 W Madison");
        assert_eq!(
            first.location,
            Coordinates {
                lat: 41.881377,
                lon: -87.668502999999
            }
        );
        assert!(first.detours_added.is_empty() && first.detours_removed.is_empty());
        assert_eq!((first.accessible, first.gtfs_sequence), (None, None));
        // Sent as "Madison & Aberdeen ".
        assert_eq!(stop(&stops, 437).name, "Madison & Aberdeen");
        assert!(stops.iter().all(|s| s.name.trim() == s.name));
    }

    #[test]
    fn unknown_stop_ids_are_left_out() {
        // 456 exists; 1577 and "sad" come back as errors about those IDs.
        let stops = parse_stops(BY_ID).unwrap();
        assert_eq!(stops.len(), 1);
        assert_eq!(stops[0].id, StopId(456));
        assert_eq!(stops[0].name, "Madison & Jefferson");
    }

    #[test]
    fn parses_detours() {
        let stops = parse_stops(DETOURS).unwrap();
        assert_eq!(stop(&stops, 8863).detours_added, [DetourId::from(DETOUR)]);
        assert!(stop(&stops, 8863).detours_removed.is_empty());
        assert_eq!(
            stop(&stops, 18299).detours_removed,
            [DetourId::from(DETOUR)]
        );
    }

    #[test]
    fn parses_documented_optional_fields() {
        let body = br#"{"bustime-response": {"stops": [
            {"stpid": "1", "stpnm": "A", "lat": 41.0, "lon": -87.0, "ada": true, "gtfsseq": 12},
            {"stpid": 2, "stpnm": "B", "lat": "41.0", "lon": "-87.0", "ada": "false", "gtfsseq": "3"}
        ]}}"#;
        let stops = parse_stops(body).unwrap();
        assert_eq!(
            (stops[0].accessible, stops[0].gtfs_sequence),
            (Some(true), Some(12))
        );
        assert_eq!(
            (stops[1].accessible, stops[1].gtfs_sequence),
            (Some(false), Some(3))
        );
        assert_eq!(
            stops[1].location,
            Coordinates {
                lat: 41.0,
                lon: -87.0
            }
        );
    }

    #[test]
    fn too_many_stops_response_is_not_valid_json() {
        // Bus Tracker's error for 11 stop IDs writes "\-", which isn't a JSON escape.
        // StopsQuery never sends 11, so only hand-built requests can get this. If this
        // test fails, Bus Tracker fixed the response and it should become an ApiError.
        assert!(matches!(
            parse_stops(TOO_MANY),
            Err(ResponseError::Decode(_))
        ));
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_stops(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
