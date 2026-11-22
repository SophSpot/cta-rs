use super::detours::{RawRouteDirection, to_detour_route};
use super::{
    Detour, DetourId, Locale, PatternId, StopId, append_locale, parse_envelope, scheduled_start,
};
use crate::deserializers::{num_or_str, one_or_many, opt_bool_or_str, opt_num_or_str};
use crate::{Coordinates, DecodeError, Request, ResponseError, chicago};
use jiff::{SignedDuration, Timestamp, ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawEnhancedDetours {
    #[serde(default, deserialize_with = "one_or_many")]
    dtrs: Vec<RawSchedule>,
}

#[derive(Deserialize)]
struct RawSchedule {
    id: String,
    #[serde(deserialize_with = "num_or_str")]
    ver: u32,
    #[serde(deserialize_with = "num_or_str")]
    st: i32,
    #[serde(default)]
    desc: String,
    #[serde(default, deserialize_with = "one_or_many")]
    rtdirs: Vec<RawRouteDirection>,
    #[serde(deserialize_with = "num_or_str")]
    startdt: i64,
    #[serde(deserialize_with = "num_or_str")]
    enddt: i64,
    #[serde(deserialize_with = "num_or_str")]
    moddt: i64,
    #[serde(default, deserialize_with = "one_or_many")]
    ptrs: Vec<RawDetourPattern>,
}

#[derive(Deserialize)]
struct RawDetourPattern {
    #[serde(deserialize_with = "num_or_str")]
    origpid: u32,
    #[serde(deserialize_with = "num_or_str")]
    dtrpid: u32,
    #[serde(default)]
    encpl: String,
    /// Not in Bus Tracker's documentation, which puts these fields directly on the
    /// pattern; CTA nests them here, one entry per detoured segment.
    #[serde(default, deserialize_with = "one_or_many")]
    modifications: Vec<RawSegment>,
    #[serde(default, deserialize_with = "one_or_many")]
    trips: Vec<RawTrip>,
}

#[derive(Deserialize)]
struct RawSegment {
    dtrstartstop: RawStop,
    #[serde(default)]
    dtrendstop: Option<RawStop>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    delay: Option<i64>,
    #[serde(default, deserialize_with = "one_or_many")]
    repstops: Vec<RawStop>,
}

#[derive(Deserialize)]
struct RawStop {
    #[serde(deserialize_with = "num_or_str")]
    geoid: u32,
    #[serde(deserialize_with = "num_or_str")]
    stpid: u32,
    #[serde(default)]
    stpnm: String,
    #[serde(deserialize_with = "num_or_str")]
    seq: u32,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    lat: Option<f64>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    lon: Option<f64>,
    #[serde(default, deserialize_with = "opt_bool_or_str")]
    adhoc: Option<bool>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    relpasstime: Option<i64>,
}

#[derive(Deserialize)]
struct RawTrip {
    #[serde(deserialize_with = "num_or_str")]
    tripid: u64,
    tatripid: String,
    origtatripno: String,
    #[serde(default, deserialize_with = "one_or_many")]
    dates: Vec<String>,
    #[serde(deserialize_with = "num_or_str")]
    stst: u32,
}

/// A detour with trip-level details, as returned by
/// [`Buses::enhanced_detours`](crate::client::Buses::enhanced_detours).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DetourSchedule {
    /// The detour, as [`detours`](crate::client::Buses::detours) also describes it.
    pub detour: Detour,
    /// When the detour was last changed.
    pub modified: ZonedDateTime,
    /// The patterns the detour changes.
    pub patterns: Vec<DetourPattern>,
}

/// One pattern a detour changes, and the trips that follow it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DetourPattern {
    /// The usual pattern the detour replaces.
    pub original: PatternId,
    /// The detour pattern, the same as a [`Pattern::id`](super::Pattern::id) from
    /// [`patterns`](crate::client::Buses::patterns).
    pub detour: PatternId,
    /// The detour pattern's path for drawing on a map, in Google's encoded polyline
    /// format. Map tools such as Google Maps accept it as is; [`decode_path`] turns it
    /// into coordinates.
    ///
    /// [`decode_path`]: DetourPattern::decode_path
    pub path: String,
    /// The stretches of the usual path the detour changes, in order.
    pub segments: Vec<DetourSegment>,
    /// The scheduled trips that follow this detour pattern.
    pub trips: Vec<DetourTrip>,
}

impl DetourPattern {
    /// [`path`](DetourPattern::path) as coordinates, start to end.
    ///
    /// The path covers the whole detour pattern, but with fewer points than
    /// [`Pattern::points`](super::Pattern::points), and no stops; use
    /// [`patterns`](crate::client::Buses::patterns) for the stops.
    ///
    /// # Errors
    ///
    /// [`DecodeError`] if the path isn't a valid encoded polyline.
    pub fn decode_path(&self) -> Result<Vec<Coordinates>, DecodeError> {
        decode_polyline(&self.path)
    }
}

/// One stretch of a pattern changed by a detour: where buses leave the usual path,
/// where they rejoin it, and the stops they use instead.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DetourSegment {
    /// The stop where buses leave the usual path.
    pub starts_at: DetourStop,
    /// The stop where buses rejoin the usual path, if Bus Tracker says.
    pub ends_at: Option<DetourStop>,
    /// How far buses are behind schedule after the detour. Negative when they come out
    /// of it early.
    pub delay: SignedDuration,
    /// The stops buses use instead of the usual ones on this stretch.
    pub replaced_stops: Vec<ReplacedStop>,
}

/// A stop where a detoured stretch starts or ends.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DetourStop {
    /// The stop's ID.
    pub stop: StopId,
    /// The stop's name, without stray spaces.
    pub name: String,
    /// The stop's position in the trip.
    pub sequence: u32,
    /// The stop's time point ID (`geoid`).
    pub timepoint: u32,
}

/// A stop a detour uses instead of the usual ones.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct ReplacedStop {
    /// The stop's ID.
    pub stop: StopId,
    /// The stop's name, without stray spaces.
    pub name: String,
    /// The stop's position in the trip.
    pub sequence: u32,
    /// The stop's time point ID (`geoid`).
    pub timepoint: u32,
    /// Where the stop is, if Bus Tracker says.
    pub location: Option<Coordinates>,
    /// Whether the stop was added just for this detour, rather than being an existing
    /// stop, if Bus Tracker says.
    pub ad_hoc: Option<bool>,
    /// How long after the previous stop buses reach this one, if Bus Tracker says.
    pub time_from_previous: Option<SignedDuration>,
}

/// A scheduled trip that follows a detour pattern.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DetourTrip {
    /// Bus Tracker's internal number for the trip (`tripid`).
    pub bustime_trip_id: u64,
    /// CTA's ID for the trip (`tatripid`), the same as
    /// [`Vehicle::trip_id`](super::Vehicle::trip_id) and
    /// [`Prediction::trip_id`](super::Prediction::trip_id).
    pub trip_id: String,
    /// The trip's ID in CTA's scheduling system (`origtatripno`).
    pub original_trip_number: String,
    /// When the trip starts, on each day it runs during the detour.
    pub starts: Vec<ZonedDateTime>,
}

/// How to list detours with trip-level details. Bus Tracker always returns every
/// detour, current and upcoming.
///
/// ```
/// use cta::buses::EnhancedDetoursQuery;
///
/// let all = EnhancedDetoursQuery::all();
/// let spanish = EnhancedDetoursQuery::all().locale("es");
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedDetoursQuery {
    locale: Option<Locale>,
}

impl EnhancedDetoursQuery {
    /// Every current and upcoming detour.
    pub fn all() -> Self {
        Self { locale: None }
    }

    /// The language for stop names and descriptions. See [`Locale`] for what CTA
    /// translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for detours with trip-level details, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn enhanced_detours_request(base: &Url, key: &str, query: &EnhancedDetoursQuery) -> Request {
    let mut url = base
        .join("getenhanceddetours")
        .expect("base URL must be http(s)");
    url.query_pairs_mut().append_pair("key", key);
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of an enhanced detours response.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_enhanced_detours(body: &[u8]) -> Result<Vec<DetourSchedule>, ResponseError> {
    let raw: RawEnhancedDetours = parse_envelope(body)?;
    let tz = chicago()?;
    Ok(raw
        .dtrs
        .into_iter()
        .map(|d| to_schedule(d, &tz))
        .collect::<Result<_, _>>()?)
}

fn epoch_ms(ms: i64, tz: &TimeZone) -> Result<ZonedDateTime, DecodeError> {
    Ok(Timestamp::from_millisecond(ms)?.to_zoned_datetime(tz.clone()))
}

fn to_schedule(d: RawSchedule, tz: &TimeZone) -> Result<DetourSchedule, DecodeError> {
    Ok(DetourSchedule {
        detour: Detour {
            id: DetourId(d.id),
            version: d.ver,
            active: d.st != 0,
            description: d.desc.trim().to_owned(),
            affects: d.rtdirs.into_iter().map(to_detour_route).collect(),
            start: epoch_ms(d.startdt, tz)?,
            end: epoch_ms(d.enddt, tz)?,
        },
        modified: epoch_ms(d.moddt, tz)?,
        patterns: d
            .ptrs
            .into_iter()
            .map(|p| to_pattern(p, tz))
            .collect::<Result<_, _>>()?,
    })
}

fn to_pattern(p: RawDetourPattern, tz: &TimeZone) -> Result<DetourPattern, DecodeError> {
    Ok(DetourPattern {
        original: PatternId(p.origpid),
        detour: PatternId(p.dtrpid),
        path: p.encpl,
        segments: p.modifications.into_iter().map(to_segment).collect(),
        trips: p
            .trips
            .into_iter()
            .map(|t| to_trip(t, tz))
            .collect::<Result<_, _>>()?,
    })
}

fn to_segment(s: RawSegment) -> DetourSegment {
    DetourSegment {
        starts_at: to_detour_stop(s.dtrstartstop),
        ends_at: s.dtrendstop.map(to_detour_stop),
        delay: SignedDuration::from_secs(s.delay.unwrap_or(0)),
        replaced_stops: s.repstops.into_iter().map(to_replaced_stop).collect(),
    }
}

fn to_detour_stop(s: RawStop) -> DetourStop {
    DetourStop {
        stop: StopId(s.stpid),
        name: s.stpnm.trim().to_owned(),
        sequence: s.seq,
        timepoint: s.geoid,
    }
}

fn to_replaced_stop(s: RawStop) -> ReplacedStop {
    ReplacedStop {
        stop: StopId(s.stpid),
        name: s.stpnm.trim().to_owned(),
        sequence: s.seq,
        timepoint: s.geoid,
        location: match (s.lat, s.lon) {
            (Some(lat), Some(lon)) => Some(Coordinates { lat, lon }),
            _ => None,
        },
        ad_hoc: s.adhoc,
        time_from_previous: s.relpasstime.map(SignedDuration::from_secs),
    }
}

fn to_trip(t: RawTrip, tz: &TimeZone) -> Result<DetourTrip, DecodeError> {
    let starts = t
        .dates
        .iter()
        .map(|d| {
            let date = civil::Date::strptime("%Y%m%d", d.trim())?;
            scheduled_start(date, t.stst, tz)
        })
        .collect::<Result<_, DecodeError>>()?;
    Ok(DetourTrip {
        bustime_trip_id: t.tripid,
        trip_id: t.tatripid,
        original_trip_number: t.origtatripno,
        starts,
    })
}

/// Decode a Google encoded polyline at precision 5, the precision Bus Tracker uses.
fn decode_polyline(s: &str) -> Result<Vec<Coordinates>, DecodeError> {
    let mut bytes = s.bytes().peekable();
    let mut coords = Vec::new();
    let (mut lat, mut lon) = (0i64, 0i64);
    while bytes.peek().is_some() {
        lat += read_polyline_value(&mut bytes)?;
        lon += read_polyline_value(&mut bytes)?;
        coords.push(Coordinates {
            lat: lat as f64 / 1e5,
            lon: lon as f64 / 1e5,
        });
    }
    Ok(coords)
}

/// Read one zigzag-encoded value: 5-bit chunks offset by 63, with 0x20 marking that
/// another chunk follows.
fn read_polyline_value(bytes: &mut impl Iterator<Item = u8>) -> Result<i64, DecodeError> {
    let invalid = || DecodeError("not a valid encoded polyline".into());
    let mut result: i64 = 0;
    let mut shift = 0;
    loop {
        let chunk = i64::from(
            bytes
                .next()
                .ok_or_else(invalid)?
                .checked_sub(63)
                .ok_or_else(invalid)?,
        );
        if chunk > 0x3f || shift > 55 {
            return Err(invalid());
        }
        result |= (chunk & 0x1f) << shift;
        shift += 5;
        if chunk < 0x20 {
            break;
        }
    }
    Ok(if result & 1 == 1 {
        !(result >> 1)
    } else {
        result >> 1
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;
    use crate::buses::{Direction, PatternPoint, Route, parse_detours, parse_patterns};

    const ENHANCED: &[u8] = include_bytes!("../../tests/fixtures/bus_enhanced_detours.json");
    const DETOURS: &[u8] = include_bytes!("../../tests/fixtures/bus_detours.json");
    const PATTERNS: &[u8] = include_bytes!("../../tests/fixtures/bus_patterns_with_detour.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn schedule<'a>(all: &'a [DetourSchedule], id: &str) -> &'a DetourSchedule {
        all.iter()
            .find(|s| s.detour.id == DetourId::from(id))
            .unwrap_or_else(|| panic!("no detour {id}"))
    }

    fn chicago_time(y: i16, mo: i8, d: i8, h: i8, mi: i8, s: i8) -> ZonedDateTime {
        civil::date(y, mo, d)
            .at(h, mi, s, 0)
            .to_zoned_datetime(chicago().unwrap())
            .unwrap()
    }

    #[test]
    fn request_url() {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url =
            enhanced_detours_request(&base, "k", &EnhancedDetoursQuery::all().locale("es")).url;
        assert_eq!(url.path(), "/bustime/api/v3/getenhanceddetours");
        assert_eq!(url.query(), Some("key=k&locale=es&format=json"));
    }

    #[test]
    fn fixture_covers_every_case() {
        // The fixture is trimmed from a live response; make sure the trimming kept these.
        let all = parse_enhanced_detours(ENHANCED).unwrap();
        let segments: Vec<&DetourSegment> = all
            .iter()
            .flat_map(|s| &s.patterns)
            .flat_map(|p| &p.segments)
            .collect();
        assert!(
            all.iter()
                .flat_map(|s| &s.patterns)
                .any(|p| p.segments.len() > 1)
        );
        assert!(segments.iter().any(|s| s.ends_at.is_none()));
        assert!(segments.iter().any(|s| s.delay.is_negative()));
        assert!(segments.iter().any(|s| !s.replaced_stops.is_empty()));
        assert!(
            all.iter()
                .flat_map(|s| &s.patterns)
                .all(|p| !p.trips.is_empty())
        );
    }

    #[test]
    fn detours_match_getdetours() {
        // Same IDs, routes, and (after converting epoch milliseconds) times.
        let plain = parse_detours(DETOURS).unwrap();
        for s in parse_enhanced_detours(ENHANCED).unwrap() {
            let d = plain.iter().find(|d| d.id == s.detour.id).unwrap();
            assert_eq!(&s.detour, d);
        }
    }

    #[test]
    fn parses_schedule() {
        let all = parse_enhanced_detours(ENHANCED).unwrap();
        let s = schedule(&all, "9874495F-F49C-4256-A2B5-29A2FB3D7501");
        assert_eq!(s.detour.affects[0].route, Route::from("50"));
        assert_eq!(s.detour.affects[0].direction, Direction::Southbound);
        assert_eq!(s.modified.timestamp().as_millisecond(), 1794598299000);
        let p = &s.patterns[0];
        assert_eq!((p.original, p.detour), (PatternId(1151), PatternId(502480)));
        assert_eq!(p.segments.len(), 6);
        assert_eq!(
            p.segments[0].starts_at,
            DetourStop {
                stop: StopId(8862),
                name: "Damen & Hubbard".into(),
                sequence: 54,
                timepoint: 8623,
            }
        );
        assert_eq!(p.segments[0].delay, SignedDuration::ZERO);
    }

    #[test]
    fn parses_segment_without_end_and_early_delay() {
        let all = parse_enhanced_detours(ENHANCED).unwrap();
        let seg = &schedule(&all, "86879503-7CEA-48C8-BB29-FFBF0D73B1EA").patterns[0].segments[0];
        assert_eq!(seg.starts_at.name, "Jeffery & 97th Street");
        assert_eq!(seg.ends_at, None);
        assert_eq!(seg.delay, SignedDuration::from_secs(-540));
    }

    #[test]
    fn parses_replaced_stops() {
        let all = parse_enhanced_detours(ENHANCED).unwrap();
        let seg = &schedule(&all, "8A3D8B0D-D95E-4621-BAF8-E969867256B5").patterns[0].segments[0];
        assert_eq!(seg.starts_at.stop, StopId(8228)); // Western & Grand
        assert_eq!(seg.ends_at.as_ref().map(|e| e.stop), Some(StopId(8231))); // Western & Lake
        assert_eq!(seg.replaced_stops.len(), 7);
        assert_eq!(
            seg.replaced_stops[0],
            ReplacedStop {
                stop: StopId(18645),
                name: "Grand & Western (Metra MD-N/MD-W/NCS)".into(),
                sequence: 36,
                timepoint: 27167,
                location: Some(Coordinates {
                    lat: 41.890572000001235,
                    lon: -87.68635199999918
                }),
                ad_hoc: Some(false),
                time_from_previous: Some(SignedDuration::from_secs(92)),
            }
        );
    }

    #[test]
    fn trips_start_on_each_date() {
        let all = parse_enhanced_detours(ENHANCED).unwrap();
        let trip = &schedule(&all, "86879503-7CEA-48C8-BB29-FFBF0D73B1EA").patterns[0].trips[0];
        assert_eq!(trip.bustime_trip_id, 47842010);
        assert_eq!(trip.trip_id, "22224290");
        assert_eq!(trip.original_trip_number, "280460044");
        // stst 53280 = 14:48 on each of the trip's five dates.
        assert_eq!(trip.starts.len(), 5);
        assert_eq!(trip.starts[0], chicago_time(2026, 11, 22, 14, 48, 0));
        assert_eq!(trip.starts[4], chicago_time(2026, 12, 13, 14, 48, 0));
    }

    #[test]
    fn decoded_path_follows_the_detour_pattern() {
        // X49's detour pattern 502458, from getenhanceddetours and from getpatterns.
        let all = parse_enhanced_detours(ENHANCED).unwrap();
        let p = &schedule(&all, "5950B28F-2CDB-4242-BDAC-B72BDB28F8F2").patterns[0];
        assert_eq!(p.detour, PatternId(502458));
        let path = p.decode_path().unwrap();

        let patterns = parse_patterns(PATTERNS).unwrap();
        let pattern = patterns.iter().find(|q| q.id == p.detour).unwrap();
        let points: Vec<Coordinates> = pattern.points.iter().map(PatternPoint::location).collect();

        let close = |a: &Coordinates, b: &Coordinates| {
            (a.lat - b.lat).abs() < 1e-4 && (a.lon - b.lon).abs() < 1e-4
        };
        assert!(close(&path[0], &points[0]));
        assert!(close(path.last().unwrap(), points.last().unwrap()));
        for c in &path {
            assert!(
                points.iter().any(|q| close(c, q)),
                "{c:?} is off the pattern"
            );
        }
    }

    #[test]
    fn decodes_reference_polyline() {
        // The example from Google's polyline documentation.
        let path = decode_polyline("_p~iF~ps|U_ulLnnqC_mqNvxq`@").unwrap();
        let expected = [(38.5, -120.2), (40.7, -120.95), (43.252, -126.453)];
        assert_eq!(path.len(), 3);
        for (c, (lat, lon)) in path.iter().zip(expected) {
            assert!(
                (c.lat - lat).abs() < 1e-9 && (c.lon - lon).abs() < 1e-9,
                "{c:?}"
            );
        }
        assert!(decode_polyline("").unwrap().is_empty());
    }

    #[test]
    fn malformed_polyline_is_error() {
        for bad in ["_p~iF", "\u{1}", "~~~~~~~~~~~~~~~~"] {
            assert!(decode_polyline(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn missing_optional_fields() {
        let body = br#"{"bustime-response": {"dtrs": [{
            "id": "X", "ver": 1, "st": 0, "desc": "d", "rtdirs": [],
            "startdt": 1794582000000, "enddt": 1794582000000, "moddt": 1794582000000,
            "ptrs": [{"origpid": 1, "dtrpid": 2, "trips": [], "modifications": [{
                "dtrstartstop": {"geoid": 1, "stpid": "5", "stpnm": "A", "seq": 1},
                "repstops": [{"geoid": 2, "stpid": "6", "stpnm": " B ", "seq": 2, "relpasstime": null}]
            }]}]
        }]}}"#;
        let s = parse_enhanced_detours(body).unwrap().remove(0);
        assert!(!s.detour.active);
        let p = &s.patterns[0];
        assert_eq!(p.path, "");
        let seg = &p.segments[0];
        assert_eq!(seg.delay, SignedDuration::ZERO);
        let r = &seg.replaced_stops[0];
        assert_eq!(r.name, "B");
        assert_eq!(
            (r.location, r.ad_hoc, r.time_from_previous),
            (None, None, None)
        );
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_enhanced_detours(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
