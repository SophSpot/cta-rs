use super::{Position, Request, Route, StationId, StopId, parse_envelope, position};
use crate::chicago; // best city in da world
use crate::deserializers::{flag, from_str, one_or_many, opt_from_str};
use crate::{DecodeError, ResponseError};
use jiff::{SignedDuration, ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use std::fmt::{Display, Formatter};
use std::num::NonZeroU32;
use url::Url;

#[derive(Deserialize)]
struct RawArrivals {
    #[serde(default, deserialize_with = "one_or_many")]
    eta: Vec<RawEta>,
}

#[derive(Deserialize)]
pub(super) struct RawEta {
    #[serde(rename = "staId", deserialize_with = "from_str")]
    sta_id: u32,
    #[serde(rename = "staNm")]
    sta_nm: String,
    #[serde(rename = "stpId", deserialize_with = "from_str")]
    stp_id: u32,
    #[serde(rename = "stpDe")]
    stp_de: String,
    rn: String,
    rt: String,
    #[serde(rename = "destSt", deserialize_with = "from_str")]
    dest_st: u32,
    #[serde(rename = "destNm")]
    dest_nm: String,
    #[serde(rename = "trDr", deserialize_with = "from_str")]
    tr_dr: u8,
    #[serde(deserialize_with = "from_str")]
    prdt: civil::DateTime,
    #[serde(rename = "arrT", deserialize_with = "from_str")]
    arr_t: civil::DateTime,
    #[serde(rename = "isApp", deserialize_with = "flag")]
    is_app: bool,
    #[serde(rename = "isSch", deserialize_with = "flag")]
    is_sch: bool,
    #[serde(rename = "isDly", deserialize_with = "flag")]
    is_dly: bool,
    #[serde(rename = "isFlt", deserialize_with = "flag")]
    is_flt: bool,
    #[serde(default, deserialize_with = "opt_from_str")]
    lat: Option<f64>,
    #[serde(default, deserialize_with = "opt_from_str")]
    lon: Option<f64>,
    #[serde(default, deserialize_with = "opt_from_str")]
    heading: Option<u16>,
}

/// A predicted arrival of one train at one platform.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Arrival {
    /// The station, e.g. `40380` for Clark/Lake.
    pub station_id: StationId,
    /// The station's name, e.g. `"Clark/Lake"`.
    pub station: String,
    /// The platform the train arrives at.
    pub stop_id: StopId,
    /// The platform's description, e.g. `"Service at Inner Loop platform"`.
    pub stop_description: String,
    /// The train's run number. Pass it to `follow` to track this train.
    ///
    /// Kept as text because the CTA uses leading zeros, e.g. `"014"`.
    pub run: String,
    /// The line the train is running on.
    pub route: Route,
    /// The stop at the end of the train's run.
    pub destination_stop: StopId,
    /// Where the train is headed, e.g. `"O'Hare"`.
    ///
    /// Occasionally a placeholder such as `"See train"`.
    pub destination: String,
    /// The CTA's direction code, `1` or `5`. Its meaning depends on the line.
    pub direction: u8,
    /// When the CTA made this prediction.
    pub predicted_at: ZonedDateTime,
    /// When the train is predicted to arrive.
    pub arrival_time: ZonedDateTime,
    /// The train is approaching or at the platform.
    pub approaching: bool,
    /// The prediction comes from the schedule, not live tracking, so it's less reliable.
    pub scheduled: bool,
    /// The train is delayed.
    pub delayed: bool,
    /// The CTA flagged a possible problem with this prediction.
    pub fault: bool,
    /// The train's last reported position, if known.
    ///
    /// Always `None` for the stops in a [`FollowedTrain`](super::FollowedTrain), which
    /// has a single position for the whole train.
    pub position: Option<Position>,
}

impl Arrival {
    /// The time from when the prediction was made until the predicted arrival.
    ///
    /// Uses the CTA's own timestamps, so it doesn't depend on the local clock.
    pub fn eta(&self) -> SignedDuration {
        self.arrival_time.duration_since(&self.predicted_at)
    }
}

/// Where to get arrivals for: a whole station or one platform.
///
/// Usually created implicitly by passing a [`StationId`] or [`StopId`] to
/// [`ArrivalsQuery::at`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Every platform at a station.
    Station(StationId),
    /// A single platform.
    Stop(StopId),
}

impl Location {
    fn param(&self) -> (&'static str, String) {
        match self {
            Location::Station(id) => ("mapid", id.to_string()),
            Location::Stop(id) => ("stpid", id.to_string()),
        }
    }
}

impl From<StationId> for Location {
    fn from(id: StationId) -> Self {
        Location::Station(id)
    }
}

impl From<StopId> for Location {
    fn from(id: StopId) -> Self {
        Location::Stop(id)
    }
}

/// Which arrivals to fetch.
///
/// ```
/// use cta::trains::{ArrivalsQuery, Route, StationId, StopId};
///
/// // Everything at Clark/Lake
/// let all = ArrivalsQuery::at(StationId(40380));
///
/// // The next 3 Brown Line trains at one platform
/// let some = ArrivalsQuery::at(StopId(30074)).route(Route::Brown).max(3);
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivalsQuery {
    location: Location,
    max: Option<NonZeroU32>,
    route: Option<Route>,
}

impl ArrivalsQuery {
    /// Arrivals at a station ([`StationId`]) or a single platform ([`StopId`]).
    pub fn at(location: impl Into<Location>) -> Self {
        Self {
            location: location.into(),
            max: None,
            route: None,
        }
    }

    /// Return at most `max` arrivals. `0` means no limit, the same as not calling this.
    pub fn max(mut self, max: u32) -> Self {
        self.max = NonZeroU32::new(max);
        self
    }

    /// Only include trains on `route`.
    pub fn route(mut self, route: Route) -> Self {
        self.route = Some(route);
        self
    }
}

impl Display for Location {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let (name, id) = self.param();
        write!(f, "{name} {id}")
    }
}

/// Build the request for an arrivals query, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn arrivals_request(base: &Url, key: &str, query: &ArrivalsQuery) -> Request {
    let mut url = base
        .join("ttarrivals.aspx")
        .expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();

        let (name, id) = query.location.param();
        q.append_pair("key", key)
            .append_pair(name, &id)
            .append_pair("outputType", "JSON");

        if let Some(max) = query.max {
            q.append_pair("max", &max.to_string());
        }
        if let Some(rt) = &query.route {
            q.append_pair("rt", rt.code());
        }
    }
    Request { url }
}

/// Parse the body of an arrivals response.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error code, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_arrivals(body: &[u8]) -> Result<Vec<Arrival>, ResponseError> {
    let raw: RawArrivals = parse_envelope(body)?;
    let tz = chicago()?;

    raw.eta
        .into_iter()
        .map(|e| to_arrival(e, &tz))
        .collect::<Result<_, DecodeError>>()
        .map_err(ResponseError::from)
}

pub(super) fn to_arrival(e: RawEta, tz: &TimeZone) -> Result<Arrival, DecodeError> {
    Ok(Arrival {
        station_id: StationId(e.sta_id),
        station: e.sta_nm,
        stop_id: StopId(e.stp_id),
        stop_description: e.stp_de,
        run: e.rn,
        route: e.rt.parse()?,
        destination_stop: StopId(e.dest_st),
        destination: e.dest_nm,
        direction: e.tr_dr,
        predicted_at: e.prdt.to_zoned_datetime(tz.clone())?,
        arrival_time: e.arr_t.to_zoned_datetime(tz.clone())?,
        approaching: e.is_app,
        scheduled: e.is_sch,
        delayed: e.is_dly,
        fault: e.is_flt,
        position: position(e.lat, e.lon, e.heading),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trains::DEFAULT_BASE_URL;

    const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/arrivals_clark_lake.json");

    #[test]
    fn parses_fixture() {
        let arrivals = parse_arrivals(FIXTURE).unwrap();
        assert_eq!(arrivals.len(), 3);

        let a = &arrivals[1];
        assert_eq!(a.station_id, StationId(40380));
        assert_eq!(a.station, "Clark/Lake");
        assert_eq!(a.stop_id, StopId(30075));
        assert_eq!(a.stop_description, "Service at Outer Loop platform");
        assert_eq!(a.run, "014"); // leading zero kept
        assert_eq!(a.route, Route::Green);
        assert_eq!(a.destination_stop, StopId(30004));
        assert_eq!(a.destination, "Harlem/Lake");
        assert_eq!(a.direction, 1);
        assert!(!a.approaching && !a.scheduled && !a.delayed && !a.fault);
        assert_eq!(a.position.unwrap().heading, 358);

        assert!(arrivals[2].approaching);
    }

    #[test]
    fn times_are_chicago_and_eta_uses_cta_clock() {
        let a = &parse_arrivals(FIXTURE).unwrap()[1];
        assert_eq!(
            a.arrival_time.to_string(),
            "2026-11-19T18:02:25-06:00[America/Chicago]"
        );
        assert_eq!(a.eta(), SignedDuration::from_secs(120));
    }

    #[test]
    fn single_eta_object_is_one_arrival() {
        let body = br#"{"ctatt":{"errCd":"0","errNm":null,"eta":{"staId":"40380","stpId":"30074","staNm":"Clark/Lake","stpDe":"x","rn":"015","rt":"G","destSt":"30139","destNm":"Cottage Grove","trDr":"5","prdt":"2026-11-19T18:00:17","arrT":"2026-11-19T18:01:17","isApp":"1","isSch":"0","isDly":"0","isFlt":"0","flags":null}}}"#;
        let arrivals = parse_arrivals(body).unwrap();
        assert_eq!(arrivals.len(), 1);
        assert_eq!(arrivals[0].position, None); // no lat/lon/heading sent
    }

    #[test]
    fn missing_or_null_eta_is_empty() {
        for body in [
            &br#"{"ctatt":{"errCd":"0","errNm":null}}"#[..],
            &br#"{"ctatt":{"errCd":"0","errNm":null,"eta":null}}"#[..],
        ] {
            assert!(parse_arrivals(body).unwrap().is_empty());
        }
    }

    #[test]
    fn unknown_route_is_decode_error() {
        let body = String::from_utf8(FIXTURE.to_vec())
            .unwrap()
            .replace(r#""rt":"Blue""#, r#""rt":"Silver""#);
        assert!(matches!(
            parse_arrivals(body.as_bytes()),
            Err(ResponseError::Decode(_))
        ));
    }

    #[test]
    fn request_for_station() {
        let base = Url::parse(DEFAULT_BASE_URL).unwrap();
        let req = arrivals_request(&base, "k", &ArrivalsQuery::at(StationId(40380)));
        assert_eq!(req.url.path(), "/api/1.0/ttarrivals.aspx");
        assert_eq!(req.url.query(), Some("key=k&mapid=40380&outputType=JSON"));
    }

    #[test]
    fn request_for_stop_with_options() {
        let base = Url::parse(DEFAULT_BASE_URL).unwrap();
        let q = ArrivalsQuery::at(StopId(30074)).max(3).route(Route::Brown);
        let req = arrivals_request(&base, "k", &q);
        assert_eq!(
            req.url.query(),
            Some("key=k&stpid=30074&outputType=JSON&max=3&rt=Brn")
        );
    }

    #[test]
    fn max_zero_sends_no_limit() {
        let base = Url::parse(DEFAULT_BASE_URL).unwrap();
        let req = arrivals_request(&base, "k", &ArrivalsQuery::at(StationId(40380)).max(0));
        assert!(!req.url.query().unwrap().contains("max="));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn arrivals_round_trip_through_json() {
        let arrivals = parse_arrivals(FIXTURE).unwrap();
        let json = serde_json::to_string(&arrivals).unwrap();
        let back: Vec<Arrival> = serde_json::from_str(&json).unwrap();
        assert_eq!(arrivals, back);
    }
}
