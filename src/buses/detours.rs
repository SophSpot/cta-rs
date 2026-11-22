use super::{DetourId, Direction, Locale, Route, append_locale, parse_envelope};
use crate::deserializers::{bus_time, num_or_str, one_or_many};
use crate::{DecodeError, Request, ResponseError, chicago};
use jiff::{ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawDetours {
    #[serde(default, deserialize_with = "one_or_many")]
    dtrs: Vec<RawDetour>,
}

#[derive(Deserialize)]
struct RawDetour {
    id: String,
    #[serde(deserialize_with = "num_or_str")]
    ver: u32,
    #[serde(deserialize_with = "num_or_str")]
    st: i32,
    #[serde(default)]
    desc: String,
    #[serde(default, deserialize_with = "one_or_many")]
    rtdirs: Vec<RawRouteDirection>,
    #[serde(deserialize_with = "bus_time")]
    startdt: civil::DateTime,
    #[serde(deserialize_with = "bus_time")]
    enddt: civil::DateTime,
}

#[derive(Deserialize)]
pub(super) struct RawRouteDirection {
    rt: String,
    dir: String,
}

pub(super) fn to_detour_route(r: RawRouteDirection) -> DetourRoute {
    DetourRoute {
        route: Route(r.rt),
        direction: Direction::from(r.dir),
    }
}

/// A detour, as listed by [`Buses::detours`](crate::client::Buses::detours).
///
/// See [Detours](super#detours) for how detours link to stops and patterns.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Detour {
    /// The detour's ID, as also found on stops and patterns.
    pub id: DetourId,
    /// The detour's version. Bus Tracker returns only the newest, and a new version
    /// can change the detour's stops and patterns.
    pub version: u32,
    /// Whether the detour is active. Canceled detours stay listed, with this `false`,
    /// because a bus may still be running one.
    pub active: bool,
    /// Bus Tracker's description of the detour. For CTA this is an internal code, such
    /// as `"050_NB_B594-26REV"`, rather than text for riders.
    pub description: String,
    /// The routes and directions the detour affects.
    pub affects: Vec<DetourRoute>,
    /// When the detour starts.
    ///
    /// Bus Tracker lists a detour for the whole service day it affects, so it can be
    /// listed before this time or after [`end`](Detour::end).
    pub start: ZonedDateTime,
    /// When the detour ends.
    pub end: ZonedDateTime,
}

/// A route and direction affected by a [`Detour`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DetourRoute {
    /// The route.
    pub route: Route,
    /// The direction.
    pub direction: Direction,
}

/// Which detours a query covers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DetoursTarget {
    All,
    Route(Route),
    RouteDirection(Route, Direction),
}

/// Which detours to list: every detour, one route's, or one route's in one direction.
///
/// ```
/// use cta::buses::{Direction, DetoursQuery};
///
/// let all = DetoursQuery::all();
/// let route = DetoursQuery::route("49");
/// let southbound = DetoursQuery::route_direction("49", Direction::Southbound);
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetoursQuery {
    target: DetoursTarget,
    locale: Option<Locale>,
}

impl DetoursQuery {
    fn new(target: DetoursTarget) -> Self {
        Self {
            target,
            locale: None,
        }
    }

    /// Every detour affecting today's service.
    pub fn all() -> Self {
        Self::new(DetoursTarget::All)
    }

    /// Detours on `route`. Bus Tracker accepts only one route per request.
    pub fn route(route: impl Into<Route>) -> Self {
        Self::new(DetoursTarget::Route(route.into()))
    }

    /// Detours on `route` going in `direction`.
    pub fn route_direction(route: impl Into<Route>, direction: impl Into<Direction>) -> Self {
        Self::new(DetoursTarget::RouteDirection(
            route.into(),
            direction.into(),
        ))
    }

    /// The language for descriptions. See [`Locale`] for what CTA translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for the detours in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn detours_request(base: &Url, key: &str, query: &DetoursQuery) -> Request {
    let mut url = base.join("getdetours").expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("key", key);
        match &query.target {
            DetoursTarget::All => {}
            DetoursTarget::Route(route) => {
                q.append_pair("rt", route.as_str());
            }
            DetoursTarget::RouteDirection(route, direction) => {
                q.append_pair("rt", route.as_str())
                    .append_pair("rtdir", direction.as_str());
            }
        }
    }
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a detours response.
///
/// An unknown route or direction has no detours rather than being reported as an error.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_detours(body: &[u8]) -> Result<Vec<Detour>, ResponseError> {
    let raw: RawDetours = parse_envelope(body)?;
    let tz = chicago()?;
    Ok(raw
        .dtrs
        .into_iter()
        .map(|d| to_detour(d, &tz))
        .collect::<Result<_, _>>()?)
}

fn to_detour(d: RawDetour, tz: &TimeZone) -> Result<Detour, DecodeError> {
    Ok(Detour {
        id: DetourId(d.id),
        version: d.ver,
        active: d.st != 0,
        description: d.desc.trim().to_owned(),
        affects: d.rtdirs.into_iter().map(to_detour_route).collect(),
        start: d.startdt.to_zoned_datetime(tz.clone())?,
        end: d.enddt.to_zoned_datetime(tz.clone())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const DETOURS: &[u8] = include_bytes!("../../tests/fixtures/bus_detours.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn query_string(query: &DetoursQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = detours_request(&base, "k", query).url;
        assert_eq!(url.path(), "/bustime/api/v3/getdetours");
        url.query().unwrap().to_owned()
    }

    fn chicago_time(y: i16, mo: i8, d: i8, h: i8, mi: i8) -> ZonedDateTime {
        civil::date(y, mo, d)
            .at(h, mi, 0, 0)
            .to_zoned_datetime(chicago().unwrap())
            .unwrap()
    }

    #[test]
    fn requests() {
        assert_eq!(query_string(&DetoursQuery::all()), "key=k&format=json");
        assert_eq!(
            query_string(&DetoursQuery::route("49").locale("es")),
            "key=k&rt=49&locale=es&format=json"
        );
        // Bus Tracker's direction matching is case-sensitive, so the canonical form is sent.
        assert_eq!(
            query_string(&DetoursQuery::route_direction("49", "southbound")),
            "key=k&rt=49&rtdir=Southbound&format=json"
        );
    }

    #[test]
    fn parses_live_response() {
        let detours = parse_detours(DETOURS).unwrap();
        assert_eq!(detours.len(), 8);
        assert!(detours.iter().all(|d| d.active));
        let d = &detours[0];
        assert_eq!(
            *d,
            Detour {
                id: DetourId::from("83D479F6-CB66-4092-9374-0659F00B1771"),
                version: 2,
                active: true,
                description: "050_NB_B594-26REV".into(),
                affects: vec![DetourRoute {
                    route: Route::from("50"),
                    direction: Direction::Northbound,
                }],
                start: chicago_time(2026, 11, 16, 9, 0),
                end: chicago_time(2026, 12, 19, 23, 50),
            }
        );
    }

    #[test]
    fn ids_match_stops_and_patterns() {
        // The same detour appears in the stops and patterns fixtures.
        let detours = parse_detours(DETOURS).unwrap();
        for id in [
            "8A3D8B0D-D95E-4621-BAF8-E969867256B5", // stops on route 49
            "5950B28F-2CDB-4242-BDAC-B72BDB28F8F2", // pattern on route X49
        ] {
            assert!(
                detours.iter().any(|d| d.id == DetourId::from(id)),
                "missing {id}"
            );
        }
    }

    #[test]
    fn parses_canceled_detour_with_several_routes() {
        // From the guide's example: a canceled detour affecting two routes.
        let body = br#"{"bustime-response": {"dtrs": [{
            "id": "329E1F2D-A848-43E9-8F90-4FB00E643786", "ver": 1, "st": 0,
            "desc": " IVD Multiroute Detour S47/62 ",
            "rtdirs": [{"rt": "800", "dir": "EASTBOUND"}, {"rt": "72M", "dir": "Northbound"}],
            "startdt": "20180404 09:06", "enddt": "20180430 03:00"
        }]}}"#;
        let d = parse_detours(body).unwrap().remove(0);
        assert!(!d.active);
        assert_eq!(d.description, "IVD Multiroute Detour S47/62");
        let affects: Vec<_> = d
            .affects
            .iter()
            .map(|a| (a.route.as_str(), a.direction.clone()))
            .collect();
        assert_eq!(
            affects,
            [
                ("800", Direction::Eastbound),
                ("72M", Direction::Northbound)
            ]
        );
    }

    #[test]
    fn unknown_route_or_direction_is_empty() {
        let body = br#"{"bustime-response": {"error": [
            {"rtdir": "southbound", "rt": "49", "msg": "Invalid parameter(s) provided"}
        ]}}"#;
        assert!(parse_detours(body).unwrap().is_empty());
        let body = br#"{"bustime-response": {"error": [
            {"rt": "nope", "msg": "Invalid parameter(s) provided"}
        ]}}"#;
        assert!(parse_detours(body).unwrap().is_empty());
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_detours(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
