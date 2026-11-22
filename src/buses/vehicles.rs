use super::{
    Locale, PassengerLoad, PatternId, QueryError, Route, StopId, VehicleId, append_locale, dedup,
    join, parse_envelope, scheduled_start,
};
use crate::deserializers::{
    bus_time, from_str, num_or_str, one_or_many, opt_bool_or_str, opt_bus_time, opt_num_or_str,
};
use crate::{Coordinates, DecodeError, Position, Request, ResponseError, chicago};
use jiff::{ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawVehicles {
    #[serde(default, deserialize_with = "one_or_many")]
    vehicle: Vec<RawVehicle>,
}

#[derive(Deserialize)]
struct RawVehicle {
    #[serde(deserialize_with = "num_or_str")]
    vid: u32,
    #[serde(deserialize_with = "bus_time")]
    tmstmp: civil::DateTime,
    #[serde(deserialize_with = "num_or_str")]
    lat: f64,
    #[serde(deserialize_with = "num_or_str")]
    lon: f64,
    #[serde(deserialize_with = "num_or_str")]
    hdg: u16,
    #[serde(deserialize_with = "num_or_str")]
    pid: u32,
    #[serde(deserialize_with = "num_or_str")]
    pdist: u32,
    rt: String,
    des: String,
    #[serde(default, deserialize_with = "opt_bool_or_str")]
    dly: Option<bool>,
    tatripid: String,
    origtatripno: String,
    tablockid: String,
    #[serde(default)]
    zone: Option<String>,
    #[serde(default)]
    psgld: Option<String>,
    #[serde(deserialize_with = "num_or_str")]
    stst: u32,
    #[serde(deserialize_with = "from_str")]
    stsd: civil::Date,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    spd: Option<u32>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    blk: Option<u32>,
    #[serde(default, deserialize_with = "opt_bus_time")]
    srvtmstmp: Option<civil::DateTime>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    stopstatus: Option<u8>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    timepointid: Option<u32>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    stopid: Option<u32>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    sequence: Option<u32>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    gtfsseq: Option<u32>,
}

/// A bus in service, as returned by [`Buses::vehicles`](crate::client::Buses::vehicles).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Vehicle {
    /// The bus's fleet number, e.g. `8932`.
    pub id: VehicleId,
    /// When the bus last reported its position.
    pub updated_at: ZonedDateTime,
    /// Where the bus was at [`updated_at`](Vehicle::updated_at), and which way it was
    /// facing.
    pub position: Position,
    /// The pattern (path) the bus is following.
    pub pattern: PatternId,
    /// How far the bus has traveled along its pattern, in feet.
    pub pattern_distance_ft: u32,
    /// The route the bus is running.
    pub route: Route,
    /// Where the bus is headed, e.g. `"Michigan/Chicago"`.
    ///
    /// Bus Tracker can translate this with [`VehiclesQuery::locale`], but CTA returns
    /// English in every locale today.
    pub destination: String,
    /// The bus has been stopped for longer than expected.
    pub delayed: bool,
    /// CTA's ID for the trip the bus is on (`tatripid`), e.g. `"10001296"`.
    pub trip_id: String,
    /// The trip's ID in CTA's scheduling system (`origtatripno`).
    pub original_trip_number: String,
    /// CTA's ID for the bus's block (`tablockid`), e.g. `"3 -712"`: the chain of trips
    /// this bus runs during the day. Kept exactly as Bus Tracker sent it.
    pub block_id: String,
    /// The named zone the bus is in, if any.
    ///
    /// CTA doesn't use zones today, so this is always `None`.
    pub zone: Option<String>,
    /// How full the bus is. Always [`PassengerLoad::Unknown`] today.
    pub passenger_load: PassengerLoad,
    /// When the bus's current trip was scheduled to start.
    pub scheduled_start: ZonedDateTime,
    /// The bus's speed, probably in miles per hour (`spd`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub speed_mph: Option<u32>,
    /// Probably Bus Tracker's internal number for the block (`blk`); see
    /// [`block_id`](Vehicle::block_id) for CTA's.
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub bustime_block: Option<u32>,
    /// A server timestamp (`srvtmstmp`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub server_time: Option<ZonedDateTime>,
    /// A stop status code (`stopstatus`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub stop_status: Option<u8>,
    /// A timepoint ID (`timepointid`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub timepoint_id: Option<u32>,
    /// A stop (`stopid`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub stop: Option<StopId>,
    /// A sequence number (`sequence`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub sequence: Option<u32>,
    /// A position in CTA's GTFS schedule data (`gtfsseq`).
    ///
    /// Documented by Bus Tracker without a description. CTA doesn't send it today, so
    /// it is always `None`.
    pub gtfs_sequence: Option<u32>,
}

/// Which buses a vehicles query covers.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VehiclesTarget {
    /// Up to 10 buses by ID. Never empty.
    Vehicles(Vec<VehicleId>),
    /// Every bus on up to 10 routes. Never empty.
    Routes(Vec<Route>),
}

/// Which buses to fetch: buses by ID, or every bus on some routes.
///
/// Requests always ask for timestamps to the second.
///
/// ```
/// use cta::buses::{VehicleId, VehiclesQuery};
///
/// let one_bus = VehiclesQuery::at(VehicleId(8932));
/// let some_buses = VehiclesQuery::vehicles([VehicleId(8932), VehicleId(7962)])?;
/// let madison = VehiclesQuery::route("20");
/// let several = VehiclesQuery::routes(["20", "X9"])?;
/// # Ok::<(), cta::buses::QueryError>(())
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehiclesQuery {
    target: VehiclesTarget,
    locale: Option<Locale>,
}

impl VehiclesQuery {
    /// The most vehicle IDs one query can cover.
    pub const MAX_VEHICLES: usize = 10;
    /// The most routes one query can cover.
    pub const MAX_ROUTES: usize = 10;

    fn new(target: VehiclesTarget) -> Self {
        Self {
            target,
            locale: None,
        }
    }

    /// One bus by ID.
    pub fn at(vehicle: VehicleId) -> Self {
        Self::new(VehiclesTarget::Vehicles(vec![vehicle]))
    }

    /// Up to 10 buses by ID in one request. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoVehicles`] if `ids` is empty, or [`QueryError::TooManyVehicles`]
    /// if it has more than 10 distinct IDs.
    pub fn vehicles(ids: impl IntoIterator<Item = VehicleId>) -> Result<Self, QueryError> {
        let ids = dedup(ids);
        match ids.len() {
            0 => Err(QueryError::NoVehicles),
            n if n > Self::MAX_VEHICLES => Err(QueryError::TooManyVehicles(n)),
            _ => Ok(Self::new(VehiclesTarget::Vehicles(ids))),
        }
    }

    /// Every bus on `route`.
    pub fn route(route: impl Into<Route>) -> Self {
        Self::new(VehiclesTarget::Routes(vec![route.into()]))
    }

    /// Every bus on up to 10 routes in one request. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoRoutes`] if `routes` is empty, or [`QueryError::TooManyRoutes`]
    /// if it has more than 10 distinct routes.
    pub fn routes<R: Into<Route>>(routes: impl IntoIterator<Item = R>) -> Result<Self, QueryError> {
        let routes = dedup(routes.into_iter().map(Into::into));
        match routes.len() {
            0 => Err(QueryError::NoRoutes),
            n if n > Self::MAX_ROUTES => Err(QueryError::TooManyRoutes(n)),
            _ => Ok(Self::new(VehiclesTarget::Routes(routes))),
        }
    }

    /// The language for destinations. See [`Locale`] for what CTA translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for the buses in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn vehicles_request(base: &Url, key: &str, query: &VehiclesQuery) -> Request {
    let mut url = base.join("getvehicles").expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("key", key);
        match &query.target {
            VehiclesTarget::Vehicles(ids) => q.append_pair("vid", &join(ids)),
            VehiclesTarget::Routes(routes) => q.append_pair("rt", &join(routes)),
        };
        q.append_pair("tmres", "s");
    }
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a vehicles response.
///
/// Routes with no buses running and vehicle IDs that aren't in service are left out,
/// not reported as errors.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_vehicles(body: &[u8]) -> Result<Vec<Vehicle>, ResponseError> {
    let raw: RawVehicles = parse_envelope(body)?;
    let tz = chicago()?;
    Ok(raw
        .vehicle
        .into_iter()
        .map(|v| to_vehicle(v, &tz))
        .collect::<Result<_, _>>()?)
}

fn to_vehicle(v: RawVehicle, tz: &TimeZone) -> Result<Vehicle, DecodeError> {
    Ok(Vehicle {
        id: VehicleId(v.vid),
        updated_at: v.tmstmp.to_zoned_datetime(tz.clone())?,
        position: Position {
            coordinates: Coordinates {
                lat: v.lat,
                lon: v.lon,
            },
            heading: v.hdg,
        },
        pattern: PatternId(v.pid),
        pattern_distance_ft: v.pdist,
        route: Route(v.rt),
        destination: v.des.trim().to_owned(),
        delayed: v.dly.unwrap_or(false),
        trip_id: v.tatripid,
        original_trip_number: v.origtatripno,
        block_id: v.tablockid,
        zone: v.zone.filter(|z| !z.trim().is_empty()),
        passenger_load: PassengerLoad::from_cta(v.psgld.as_deref().unwrap_or("")),
        scheduled_start: scheduled_start(v.stsd, v.stst, tz)?,
        speed_mph: v.spd,
        bustime_block: v.blk,
        server_time: v
            .srvtmstmp
            .map(|t| t.to_zoned_datetime(tz.clone()))
            .transpose()?,
        stop_status: v.stopstatus,
        timepoint_id: v.timepointid,
        stop: v.stopid.map(StopId),
        sequence: v.sequence,
        gtfs_sequence: v.gtfsseq,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const BY_ROUTE: &[u8] = include_bytes!("../../tests/fixtures/bus_vehicles_by_route.json");
    const BY_ID: &[u8] = include_bytes!("../../tests/fixtures/bus_vehicles_by_id.json");
    const TOO_MANY: &[u8] = include_bytes!("../../tests/fixtures/bus_vehicles_too_many.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn query_string(query: &VehiclesQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = vehicles_request(&base, "k", query).url;
        assert_eq!(url.path(), "/bustime/api/v3/getvehicles");
        url.query().unwrap().to_owned()
    }

    fn chicago_time(y: i16, mo: i8, d: i8, h: i8, mi: i8, s: i8) -> ZonedDateTime {
        civil::date(y, mo, d)
            .at(h, mi, s, 0)
            .to_zoned_datetime(chicago().unwrap())
            .unwrap()
    }

    #[test]
    fn requests_always_ask_for_seconds() {
        assert_eq!(
            query_string(&VehiclesQuery::at(VehicleId(8932))),
            "key=k&vid=8932&tmres=s&format=json"
        );
        let q = VehiclesQuery::vehicles([VehicleId(8932), VehicleId(7962)]).unwrap();
        assert_eq!(
            query_string(&q),
            "key=k&vid=8932%2C7962&tmres=s&format=json"
        );
        assert_eq!(
            query_string(&VehiclesQuery::route("20").locale("es")),
            "key=k&rt=20&tmres=s&locale=es&format=json"
        );
        let q = VehiclesQuery::routes(["20", "X9", "20"]).unwrap();
        assert_eq!(query_string(&q), "key=k&rt=20%2CX9&tmres=s&format=json");
    }

    #[test]
    fn limits() {
        assert_eq!(VehiclesQuery::vehicles([]), Err(QueryError::NoVehicles));
        assert!(VehiclesQuery::vehicles((1..=10).map(VehicleId)).is_ok());
        assert_eq!(
            VehiclesQuery::vehicles((1..=11).map(VehicleId)),
            Err(QueryError::TooManyVehicles(11))
        );
        assert_eq!(
            VehiclesQuery::routes(Vec::<Route>::new()),
            Err(QueryError::NoRoutes)
        );
        assert!(VehiclesQuery::routes((1..=10).map(|n| n.to_string())).is_ok());
        assert_eq!(
            VehiclesQuery::routes((1..=11).map(|n| n.to_string())),
            Err(QueryError::TooManyRoutes(11))
        );
        // Duplicates don't count toward the limit.
        assert!(VehiclesQuery::routes((1..=10).chain(1..=5).map(|n| n.to_string())).is_ok());
    }

    #[test]
    fn parses_live_response() {
        let vehicles = parse_vehicles(BY_ROUTE).unwrap();
        assert_eq!(vehicles.len(), 61);
        let v = &vehicles[0];
        assert_eq!(v.id, VehicleId(8932));
        assert_eq!(v.updated_at, chicago_time(2026, 11, 22, 12, 34, 2));
        assert_eq!(
            v.position,
            Position {
                coordinates: Coordinates {
                    lat: 41.72074890136719,
                    lon: -87.62371826171875
                },
                heading: 0
            }
        );
        assert_eq!(v.pattern, PatternId(18414));
        assert_eq!(v.pattern_distance_ft, 0);
        assert_eq!(v.route, Route::from("3"));
        assert_eq!(v.destination, "Michigan/Chicago");
        assert!(!v.delayed);
        assert_eq!(v.trip_id, "10001296");
        assert_eq!(v.original_trip_number, "280451146");
        assert_eq!(v.block_id, "3 -712");
        assert_eq!(v.zone, None);
        assert_eq!(v.passenger_load, PassengerLoad::Unknown); // "N/A"
        assert_eq!(v.scheduled_start, chicago_time(2026, 11, 22, 12, 43, 30));
        // Documented, but not sent by CTA.
        assert_eq!(
            (v.speed_mph, v.bustime_block, v.server_time.as_ref(), v.stop),
            (None, None, None, None)
        );
    }

    #[test]
    fn unknown_vehicle_ids_are_left_out() {
        // 999999 comes back as an error about that ID.
        let ids: Vec<_> = parse_vehicles(BY_ID)
            .unwrap()
            .iter()
            .map(|v| v.id)
            .collect();
        assert_eq!(ids, [VehicleId(8932), VehicleId(7962)]);
    }

    #[test]
    fn parses_documented_fields_cta_does_not_send() {
        let body = br#"{"bustime-response": {"vehicle": [{
            "vid": "1", "tmstmp": "20261122 12:00:00", "lat": 41.0, "lon": -87.0, "hdg": 90,
            "pid": 5, "pdist": 100, "rt": "20", "des": " Austin ", "dly": true,
            "tatripid": "t", "origtatripno": "o", "tablockid": "b", "zone": "Bay 1",
            "psgld": "HALF_EMPTY", "stst": 43200, "stsd": "2026-11-22",
            "spd": 27, "blk": "701", "srvtmstmp": "20261122 12:00:05", "stopstatus": 2,
            "timepointid": "17", "stopid": "456", "sequence": 3, "gtfsseq": "4"
        }]}}"#;
        let v = parse_vehicles(body).unwrap().remove(0);
        assert_eq!(v.destination, "Austin");
        assert!(v.delayed);
        assert_eq!(v.zone.as_deref(), Some("Bay 1"));
        assert_eq!(v.passenger_load, PassengerLoad::HalfEmpty);
        assert_eq!(v.speed_mph, Some(27));
        assert_eq!(v.bustime_block, Some(701));
        assert_eq!(v.server_time, Some(chicago_time(2026, 11, 22, 12, 0, 5)));
        assert_eq!(v.stop_status, Some(2));
        assert_eq!(v.timepoint_id, Some(17));
        assert_eq!(v.stop, Some(StopId(456)));
        assert_eq!((v.sequence, v.gtfs_sequence), (Some(3), Some(4)));
    }

    #[test]
    fn missing_delay_means_not_delayed() {
        // The guide says `dly` is only present when the bus is delayed.
        let body = br#"{"bustime-response": {"vehicle": [{
            "vid": "1", "tmstmp": "20261122 12:00", "lat": "41", "lon": "-87", "hdg": "0",
            "pid": 5, "pdist": 0, "rt": "20", "des": "Austin", "tatripid": "t",
            "origtatripno": "o", "tablockid": "b", "zone": "", "psgld": "",
            "stst": 43200, "stsd": "2026-11-22"
        }]}}"#;
        let v = parse_vehicles(body).unwrap().remove(0);
        assert!(!v.delayed);
        assert_eq!(v.passenger_load, PassengerLoad::Unknown);
    }

    #[test]
    fn too_many_is_api_error() {
        // What a hand-built request with 11 routes gets; VehiclesQuery never sends one.
        match parse_vehicles(TOO_MANY) {
            Err(ResponseError::Api(e)) => {
                assert_eq!(e.kind(), ApiErrorKind::Other);
                assert_eq!(e.message, "Maximum number of rt identifiers exceeded");
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_vehicles(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
