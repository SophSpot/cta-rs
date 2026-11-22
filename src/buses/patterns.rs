use super::{
    DetourId, Direction, Locale, PatternId, QueryError, Route, StopId, append_locale, dedup, join,
    parse_envelope,
};
use crate::deserializers::{num_or_str, one_or_many, opt_num_or_str};
use crate::{Coordinates, DecodeError, Request, ResponseError};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawPatterns {
    #[serde(default, deserialize_with = "one_or_many")]
    ptr: Vec<RawPattern>,
}

#[derive(Deserialize)]
struct RawPattern {
    #[serde(deserialize_with = "num_or_str")]
    pid: u32,
    #[serde(deserialize_with = "num_or_str")]
    ln: f64,
    rtdir: String,
    #[serde(default, deserialize_with = "one_or_many")]
    pt: Vec<RawPoint>,
    #[serde(default)]
    dtrid: Option<String>,
    #[serde(default, deserialize_with = "one_or_many")]
    dtrpt: Vec<RawPoint>,
}

#[derive(Deserialize)]
struct RawPoint {
    #[serde(deserialize_with = "num_or_str")]
    seq: u32,
    typ: String,
    #[serde(deserialize_with = "num_or_str")]
    lat: f64,
    #[serde(deserialize_with = "num_or_str")]
    lon: f64,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    stpid: Option<u32>,
    #[serde(default)]
    stpnm: Option<String>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    pdist: Option<f64>,
}

/// One path a route takes, as returned by
/// [`Buses::patterns`](crate::client::Buses::patterns): its stops in order, and the
/// points between them for drawing it on a map.
///
/// Bus Tracker doesn't say which route a pattern belongs to. Vehicles and predictions
/// give both a route and a pattern.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Pattern {
    /// The pattern's ID.
    pub id: PatternId,
    /// The pattern's length in feet.
    pub length_ft: f64,
    /// The direction buses on this pattern go.
    pub direction: Direction,
    /// The pattern's stops and waypoints, in order.
    pub points: Vec<PatternPoint>,
    /// The detour that created this pattern, if it's a temporary detour pattern.
    pub detour: Option<DetourId>,
    /// For a detour pattern, the path it temporarily replaces, in order. Empty for
    /// other patterns.
    pub original_points: Vec<PatternPoint>,
}

impl Pattern {
    /// The pattern's stops, in order.
    pub fn stops(&self) -> impl Iterator<Item = StopId> + '_ {
        self.points.iter().filter_map(PatternPoint::stop)
    }
}

/// A point along a [`Pattern`]: a stop, or a waypoint that shapes the path between stops.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PatternPoint {
    /// A stop.
    Stop {
        /// The stop's ID.
        stop: StopId,
        /// The stop's name, without the stray spaces Bus Tracker sometimes adds.
        ///
        /// Bus Tracker can translate this with [`PatternsQuery::locale`], but CTA
        /// returns English in every locale today.
        name: String,
        /// Where the stop is.
        location: Coordinates,
        /// How far along the pattern the stop is, in feet.
        distance_ft: f64,
    },
    /// A point that shapes the path between stops, such as a corner.
    Waypoint {
        /// Where the point is.
        location: Coordinates,
    },
}

impl PatternPoint {
    /// Where the point is, for either kind.
    pub fn location(&self) -> Coordinates {
        match self {
            PatternPoint::Stop { location, .. } | PatternPoint::Waypoint { location } => *location,
        }
    }

    /// The stop's ID, or `None` for a waypoint.
    pub fn stop(&self) -> Option<StopId> {
        match self {
            PatternPoint::Stop { stop, .. } => Some(*stop),
            PatternPoint::Waypoint { .. } => None,
        }
    }
}

/// Which patterns a query covers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternsTarget {
    /// Every active pattern on one route.
    Route(Route),
    /// Up to 10 patterns by ID. Never empty.
    Patterns(Vec<PatternId>),
}

/// Which patterns to fetch: every active pattern on one route, or patterns by ID.
///
/// A route's active patterns are its usual ones plus any that buses are running now,
/// such as detours.
///
/// ```
/// use cta::buses::{PatternId, PatternsQuery};
///
/// let madison = PatternsQuery::route("20");
/// let one = PatternsQuery::pattern(PatternId(954));
/// let some = PatternsQuery::patterns([PatternId(954), PatternId(957)])?;
/// # Ok::<(), cta::buses::QueryError>(())
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternsQuery {
    target: PatternsTarget,
    locale: Option<Locale>,
}

impl PatternsQuery {
    /// The most pattern IDs one query can cover.
    pub const MAX_PATTERNS: usize = 10;

    fn new(target: PatternsTarget) -> Self {
        Self {
            target,
            locale: None,
        }
    }

    /// Every active pattern on `route`. Bus Tracker accepts only one route per request.
    pub fn route(route: impl Into<Route>) -> Self {
        Self::new(PatternsTarget::Route(route.into()))
    }

    /// One pattern by ID.
    pub fn pattern(pattern: PatternId) -> Self {
        Self::new(PatternsTarget::Patterns(vec![pattern]))
    }

    /// Up to 10 patterns by ID in one request. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoPatterns`] if `ids` is empty, or [`QueryError::TooManyPatterns`]
    /// if it has more than 10 distinct IDs.
    pub fn patterns(ids: impl IntoIterator<Item = PatternId>) -> Result<Self, QueryError> {
        let ids = dedup(ids);
        match ids.len() {
            0 => Err(QueryError::NoPatterns),
            n if n > Self::MAX_PATTERNS => Err(QueryError::TooManyPatterns(n)),
            _ => Ok(Self::new(PatternsTarget::Patterns(ids))),
        }
    }

    /// The language for stop names. See [`Locale`] for what CTA translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for the patterns in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn patterns_request(base: &Url, key: &str, query: &PatternsQuery) -> Request {
    let mut url = base.join("getpatterns").expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("key", key);
        match &query.target {
            PatternsTarget::Route(route) => q.append_pair("rt", route.as_str()),
            PatternsTarget::Patterns(ids) => q.append_pair("pid", &join(ids)),
        };
    }
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a patterns response.
///
/// Unknown routes and pattern IDs are left out, not reported as errors.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_patterns(body: &[u8]) -> Result<Vec<Pattern>, ResponseError> {
    let raw: RawPatterns = parse_envelope(body)?;
    Ok(raw
        .ptr
        .into_iter()
        .map(to_pattern)
        .collect::<Result<_, _>>()?)
}

fn to_pattern(p: RawPattern) -> Result<Pattern, DecodeError> {
    Ok(Pattern {
        id: PatternId(p.pid),
        length_ft: p.ln,
        direction: Direction::from(p.rtdir),
        points: to_points(p.pt)?,
        detour: p.dtrid.filter(|d| !d.trim().is_empty()).map(DetourId),
        original_points: to_points(p.dtrpt)?,
    })
}

fn to_points(mut raw: Vec<RawPoint>) -> Result<Vec<PatternPoint>, DecodeError> {
    raw.sort_by_key(|p| p.seq);
    raw.into_iter().map(to_point).collect()
}

fn to_point(p: RawPoint) -> Result<PatternPoint, DecodeError> {
    let location = Coordinates {
        lat: p.lat,
        lon: p.lon,
    };
    if p.typ.trim().eq_ignore_ascii_case("S") {
        let stop = p.stpid.ok_or_else(|| {
            DecodeError(format!("pattern stop at seq {} has no stpid", p.seq).into())
        })?;
        Ok(PatternPoint::Stop {
            stop: StopId(stop),
            name: p.stpnm.unwrap_or_default().trim().to_owned(),
            location,
            distance_ft: p.pdist.unwrap_or(0.0),
        })
    } else {
        Ok(PatternPoint::Waypoint { location })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const MADISON: &[u8] = include_bytes!("../../tests/fixtures/bus_patterns_20.json");
    const DETOUR: &[u8] = include_bytes!("../../tests/fixtures/bus_patterns_with_detour.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn query_string(query: &PatternsQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = patterns_request(&base, "k", query).url;
        assert_eq!(url.path(), "/bustime/api/v3/getpatterns");
        url.query().unwrap().to_owned()
    }

    fn point(lat: f64, lon: f64) -> Coordinates {
        Coordinates { lat, lon }
    }

    #[test]
    fn requests() {
        assert_eq!(
            query_string(&PatternsQuery::route("20")),
            "key=k&rt=20&format=json"
        );
        assert_eq!(
            query_string(&PatternsQuery::pattern(PatternId(954)).locale("es")),
            "key=k&pid=954&locale=es&format=json"
        );
        let q = PatternsQuery::patterns([PatternId(954), PatternId(957), PatternId(954)]).unwrap();
        assert_eq!(query_string(&q), "key=k&pid=954%2C957&format=json");
    }

    #[test]
    fn limits() {
        assert_eq!(PatternsQuery::patterns([]), Err(QueryError::NoPatterns));
        assert!(PatternsQuery::patterns((1..=10).map(PatternId)).is_ok());
        assert_eq!(
            PatternsQuery::patterns((1..=11).map(PatternId)),
            Err(QueryError::TooManyPatterns(11))
        );
    }

    #[test]
    fn parses_route_patterns() {
        let patterns = parse_patterns(MADISON).unwrap();
        let ids: Vec<_> = patterns.iter().map(|p| p.id).collect();
        assert_eq!(ids, [PatternId(954), PatternId(957)]);

        let west = &patterns[0];
        assert_eq!(west.direction, Direction::Westbound);
        assert_eq!(west.length_ft, 41096.0);
        assert_eq!(west.points.len(), 391);
        assert_eq!(west.detour, None);
        assert!(west.original_points.is_empty());
        // The first points, as Bus Tracker sent them: two waypoints, then a stop.
        assert_eq!(
            west.points[0],
            PatternPoint::Waypoint {
                location: point(41.882155, -87.625147000001)
            }
        );
        assert_eq!(
            west.points[2],
            PatternPoint::Stop {
                stop: StopId(450),
                name: "Madison & Wabash".into(),
                location: point(41.882129000001, -87.625855),
                distance_ft: 194.0,
            }
        );
        assert_eq!(west.stops().count(), 60);
        assert_eq!(west.stops().last(), Some(StopId(386))); // Madison & Austin Terminal
        assert_eq!(
            west.points[2].location(),
            point(41.882129000001, -87.625855)
        );
        assert_eq!(west.points[0].stop(), None);
    }

    #[test]
    fn stop_names_are_trimmed_and_distances_increase() {
        for pattern in parse_patterns(MADISON).unwrap() {
            let mut last = -1.0;
            for p in &pattern.points {
                if let PatternPoint::Stop {
                    name, distance_ft, ..
                } = p
                {
                    assert_eq!(name.trim(), name);
                    assert!(*distance_ft >= last, "{name}");
                    last = *distance_ft;
                }
            }
        }
    }

    #[test]
    fn parses_detour_pattern() {
        let patterns = parse_patterns(DETOUR).unwrap();
        let detour = patterns.iter().find(|p| p.detour.is_some()).unwrap();
        assert_eq!(detour.id, PatternId(502458));
        assert_eq!(
            detour.detour,
            Some(DetourId::from("5950B28F-2CDB-4242-BDAC-B72BDB28F8F2"))
        );
        assert_eq!(detour.length_ft, 89383.37);
        assert_eq!(detour.points.len(), 244);
        assert_eq!(detour.original_points.len(), 224);
        assert_eq!(detour.stops().count(), 37);
        // The original path starts at the same terminal.
        assert_eq!(detour.original_points[0].stop(), Some(StopId(14182)));
        assert!(patterns.iter().any(|p| p.detour.is_none()));
    }

    #[test]
    fn points_are_put_in_order() {
        let body = br#"{"bustime-response": {"ptr": [{"pid": 1, "ln": 10.0, "rtdir": "Eastbound", "pt": [
            {"seq": 3, "typ": "W", "lat": 3.0, "lon": 3.0, "pdist": 0.0},
            {"seq": 1, "typ": "S", "lat": 1.0, "lon": 1.0, "stpid": "5", "stpnm": "A ", "pdist": 0.0},
            {"seq": 2, "typ": "W", "lat": 2.0, "lon": 2.0, "pdist": 0.0}
        ]}]}}"#;
        let p = parse_patterns(body).unwrap().remove(0);
        let lats: Vec<_> = p.points.iter().map(|pt| pt.location().lat).collect();
        assert_eq!(lats, [1.0, 2.0, 3.0]);
        assert_eq!(p.points[0].stop(), Some(StopId(5)));
    }

    #[test]
    fn unknown_ids_are_left_out() {
        let body = br#"{"bustime-response": {
            "ptr": [{"pid": 954, "ln": 1.0, "rtdir": "Westbound", "pt": []}],
            "error": [{"pid": "1", "msg": "No data found for parameter"}]
        }}"#;
        let ids: Vec<_> = parse_patterns(body).unwrap().iter().map(|p| p.id).collect();
        assert_eq!(ids, [PatternId(954)]);
        // A list of routes is treated as one unknown route.
        let body = br#"{"bustime-response": {"error": [{"rt": "20,X9", "msg": "No data found for parameter"}]}}"#;
        assert!(parse_patterns(body).unwrap().is_empty());
    }

    #[test]
    fn stop_without_id_is_decode_error() {
        let body = br#"{"bustime-response": {"ptr": [{"pid": 1, "ln": 1.0, "rtdir": "Eastbound",
            "pt": [{"seq": 1, "typ": "S", "lat": 1.0, "lon": 1.0}]}]}}"#;
        assert!(matches!(
            parse_patterns(body),
            Err(ResponseError::Decode(_))
        ));
    }

    #[test]
    fn request_errors_are_api_errors() {
        let body = br#"{"bustime-response": {"error": [{"msg": "Maximum number of pid identifiers exceeded"}]}}"#;
        assert!(matches!(parse_patterns(body), Err(ResponseError::Api(_))));
        assert!(matches!(
            parse_patterns(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
