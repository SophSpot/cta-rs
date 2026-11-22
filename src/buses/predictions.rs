use super::{
    Direction, Locale, PassengerLoad, QueryError, Route, StopId, VehicleId, append_locale, dedup,
    join, parse_envelope, scheduled_start,
};
use crate::deserializers::{
    bus_time, from_str, num_or_str, one_or_many, opt_bool_or_str, opt_num_or_str,
};
use crate::{DecodeError, Request, ResponseError, chicago};
use jiff::{SignedDuration, ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawPredictions {
    #[serde(default, deserialize_with = "one_or_many")]
    prd: Vec<RawPrediction>,
}

#[derive(Deserialize)]
struct RawPrediction {
    #[serde(deserialize_with = "bus_time")]
    tmstmp: civil::DateTime,
    typ: String,
    stpnm: String,
    #[serde(deserialize_with = "num_or_str")]
    stpid: u32,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    vid: Option<u32>,
    #[serde(deserialize_with = "num_or_str")]
    dstp: u32,
    rt: String,
    rtdd: String,
    rtdir: String,
    des: String,
    #[serde(deserialize_with = "bus_time")]
    prdtm: civil::DateTime,
    #[serde(default, deserialize_with = "opt_bool_or_str")]
    dly: Option<bool>,
    #[serde(default, rename = "dyn", deserialize_with = "opt_num_or_str")]
    dynamic: Option<u8>,
    tablockid: String,
    tatripid: String,
    origtatripno: String,
    prdctdn: String,
    #[serde(default)]
    zone: Option<String>,
    #[serde(default)]
    psgld: Option<String>,
    #[serde(deserialize_with = "num_or_str")]
    stst: u32,
    #[serde(deserialize_with = "from_str")]
    stsd: civil::Date,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    flagstop: Option<i32>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    gtfsseq: Option<u32>,
    #[serde(default)]
    nbus: Option<String>,
}

/// A predicted arrival or departure of one bus at one stop, as returned by
/// [`Buses::predictions`](crate::client::Buses::predictions).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Prediction {
    /// When Bus Tracker last calculated this prediction.
    ///
    /// Predictions in one response can be several minutes old, so don't measure the
    /// time remaining from this; use [`eta_from`](Prediction::eta_from).
    pub predicted_at: ZonedDateTime,
    /// When the bus is predicted to arrive at the stop, or to leave it if
    /// [`kind`](Prediction::kind) is [`Departure`](PredictionKind::Departure).
    ///
    /// Already past for some canceled trips.
    pub time: ZonedDateTime,
    /// Whether [`time`](Prediction::time) is an arrival or a departure.
    pub kind: PredictionKind,
    /// The stop.
    pub stop: StopId,
    /// The stop's name, without the stray spaces Bus Tracker sometimes adds.
    ///
    /// Bus Tracker can translate this with a query's `locale`, but CTA returns English
    /// in every locale today.
    pub stop_name: String,
    /// The bus, if one is assigned. `None` for canceled trips.
    pub vehicle: Option<VehicleId>,
    /// How far the bus still has to travel to reach the stop, in feet.
    pub distance_ft: u32,
    /// The route.
    pub route: Route,
    /// The route's label for showing to riders. The same as
    /// [`route`](Prediction::route) for every CTA route today.
    pub route_display: String,
    /// The direction the bus is going.
    pub direction: Direction,
    /// Where the bus is headed, e.g. `"Austin"`.
    ///
    /// Bus Tracker can translate this with a query's `locale`, but CTA returns English
    /// in every locale today.
    pub destination: String,
    /// The bus is delayed. [`countdown`](Prediction::countdown) is then
    /// [`Countdown::Delayed`].
    pub delayed: bool,
    /// A schedule change affecting this prediction, such as a cancellation.
    pub dynamic_action: DynamicAction,
    /// CTA's ID for the trip (`tatripid`), e.g. `"1W-00021"`.
    pub trip_id: String,
    /// The trip's ID in CTA's scheduling system (`origtatripno`).
    pub original_trip_number: String,
    /// CTA's ID for the bus's block (`tablockid`), e.g. `"20 -805"`: the chain of trips
    /// one bus runs during the day. Kept exactly as Bus Tracker sent it.
    pub block_id: String,
    /// The named zone the bus is in, if any.
    ///
    /// CTA doesn't use zones today, so this is always `None`.
    pub zone: Option<String>,
    /// How full the bus is. Always [`PassengerLoad::Unknown`] today.
    pub passenger_load: PassengerLoad,
    /// When the trip was scheduled to start.
    pub scheduled_start: ZonedDateTime,
    /// Whether this is a flag stop, where the bus stops only on request.
    pub flag_stop: FlagStop,
    /// Bus Tracker's countdown, the value CTA's signs show, as of when the response was
    /// sent.
    pub countdown: Countdown,
    /// A position in CTA's GTFS schedule data (`gtfsseq`).
    ///
    /// Bus Tracker documents this field, but CTA doesn't send it today, so it is
    /// always `None`.
    pub gtfs_sequence: Option<u32>,
    /// Bus Tracker's `nbus` field. Its meaning is unknown: the guide lists it without a
    /// description.
    ///
    /// CTA doesn't send it today, so it is always `None`.
    pub nbus: Option<String>,
}

impl Prediction {
    /// The time from `now` until [`time`](Prediction::time). Negative once it has
    /// passed.
    ///
    /// For `now`, use Bus Tracker's clock ([`Buses::time`](crate::client::Buses::time))
    /// or a local clock you trust. Don't use
    /// [`predicted_at`](Prediction::predicted_at): predictions can be minutes old, and
    /// measuring from it overstates the time left.
    ///
    /// ```
    /// # fn run(p: &cta::buses::Prediction) {
    /// let left = p.eta_from(&jiff::ZonedDateTime::now());
    /// println!("{} min", left.as_secs() / 60);
    /// # }
    /// ```
    pub fn eta_from(&self, now: &ZonedDateTime) -> SignedDuration {
        self.time.duration_since(now)
    }
}

/// Whether a prediction is for a bus arriving at a stop or leaving it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PredictionKind {
    /// The bus is predicted to arrive at the stop (`typ` `A`).
    Arrival,
    /// The bus is predicted to leave the stop (`typ` `D`), such as at the start of a
    /// trip or after a layover.
    Departure,
    /// A type other than the two above, as Bus Tracker sent it.
    Other(String),
}

impl PredictionKind {
    fn from_cta(s: &str) -> Self {
        match s.trim() {
            t if t.eq_ignore_ascii_case("A") => PredictionKind::Arrival,
            t if t.eq_ignore_ascii_case("D") => PredictionKind::Departure,
            _ => PredictionKind::Other(s.to_owned()),
        }
    }
}

/// A schedule change affecting a prediction (Bus Tracker's `dyn`).
///
/// Most predictions are [`DynamicAction::None`]. Use the helpers for the common
/// questions rather than matching every variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum DynamicAction {
    /// No change (0).
    None,
    /// The stop or trip has been canceled (1).
    Canceled,
    /// The stop or trip has been moved to a different bus or operator (2).
    Reassigned,
    /// The time of the stop or trip has been moved (3).
    Shifted,
    /// Drop-off only: the bus won't stop to pick up riders (4).
    Expressed,
    /// Some stops on the trip changed, but the trip itself didn't (6).
    StopsAffected,
    /// A trip added on the fly; not in the schedule (8).
    NewTrip,
    /// The trip was split or short-turned, and this part keeps the original IDs (9).
    PartialTrip,
    /// The trip was split, and this part has new IDs (10).
    PartialTripNew,
    /// Canceled, but the cancellation shouldn't be shown to riders (12).
    DelayedCancel,
    /// A stop that wasn't originally scheduled (13).
    AddedStop,
    /// The trip has been delayed (14).
    UnknownDelay,
    /// A trip added on the fly has been delayed (15).
    UnknownDelayNew,
    /// The trip has been invalidated; its predictions shouldn't be shown to riders (16).
    InvalidatedTrip,
    /// A trip added on the fly has been invalidated; its predictions shouldn't be shown
    /// to riders (17).
    InvalidatedTripNew,
    /// A trip added on the fly has been canceled (18).
    CancelledTripNew,
    /// Some stops on a trip added on the fly changed, but the trip itself didn't (19).
    StopsAffectedNew,
    /// A code Bus Tracker doesn't document.
    Other(u8),
}

impl DynamicAction {
    fn from_code(code: u8) -> Self {
        use DynamicAction::*;
        match code {
            0 => None,
            1 => Canceled,
            2 => Reassigned,
            3 => Shifted,
            4 => Expressed,
            6 => StopsAffected,
            8 => NewTrip,
            9 => PartialTrip,
            10 => PartialTripNew,
            12 => DelayedCancel,
            13 => AddedStop,
            14 => UnknownDelay,
            15 => UnknownDelayNew,
            16 => InvalidatedTrip,
            17 => InvalidatedTripNew,
            18 => CancelledTripNew,
            19 => StopsAffectedNew,
            n => Other(n),
        }
    }

    /// The trip or stop is canceled: [`Canceled`](Self::Canceled),
    /// [`DelayedCancel`](Self::DelayedCancel), or
    /// [`CancelledTripNew`](Self::CancelledTripNew).
    pub fn is_canceled(self) -> bool {
        matches!(
            self,
            Self::Canceled | Self::DelayedCancel | Self::CancelledTripNew
        )
    }

    /// The bus will only let riders off here: [`Expressed`](Self::Expressed).
    pub fn is_drop_off_only(self) -> bool {
        self == Self::Expressed
    }

    /// CTA asks that this prediction not be shown to riders:
    /// [`DelayedCancel`](Self::DelayedCancel), [`InvalidatedTrip`](Self::InvalidatedTrip),
    /// or [`InvalidatedTripNew`](Self::InvalidatedTripNew).
    ///
    /// This library returns these predictions anyway; apps for riders should usually
    /// skip them.
    pub fn is_hidden(self) -> bool {
        matches!(
            self,
            Self::DelayedCancel | Self::InvalidatedTrip | Self::InvalidatedTripNew
        )
    }
}

/// Whether a stop is a flag stop, where the bus stops only on request (`flagstop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum FlagStop {
    /// No flag-stop information (-1).
    Undefined,
    /// A normal stop (0).
    Normal,
    /// A flag stop for picking up and letting off riders (1).
    PickupAndDischarge,
    /// A flag stop for letting off riders only (2).
    DischargeOnly,
    /// A code Bus Tracker doesn't document.
    Other(i32),
}

impl FlagStop {
    fn from_code(code: i32) -> Self {
        match code {
            -1 => FlagStop::Undefined,
            0 => FlagStop::Normal,
            1 => FlagStop::PickupAndDischarge,
            2 => FlagStop::DischargeOnly,
            n => FlagStop::Other(n),
        }
    }
}

/// Bus Tracker's countdown (`prdctdn`), the value CTA's signs show.
///
/// It counts down from when the response was sent, so it's correct as of then without
/// needing a clock. For an exact or live-updating time, use
/// [`Prediction::eta_from`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Countdown {
    /// The bus is due: less than about 2 minutes away (`"DUE"`).
    Due,
    /// The bus is delayed (`"DLY"`).
    Delayed,
    /// Whole minutes until the bus, rounded down. Negative for some canceled trips.
    Minutes(i32),
    /// A value other than the above, as Bus Tracker sent it.
    Other(String),
}

impl Countdown {
    fn from_cta(s: &str) -> Self {
        let t = s.trim();
        if t.eq_ignore_ascii_case("DUE") {
            Countdown::Due
        } else if t.eq_ignore_ascii_case("DLY") {
            Countdown::Delayed
        } else if let Ok(n) = t.parse() {
            Countdown::Minutes(n)
        } else {
            Countdown::Other(s.to_owned())
        }
    }
}

/// Predictions for stops, built by [`PredictionsQuery::at`] or
/// [`PredictionsQuery::stops`]. Can be limited to some routes.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopPredictions {
    stops: Vec<StopId>,
    routes: Vec<Route>,
    top: Option<u32>,
    locale: Option<Locale>,
}

impl StopPredictions {
    /// Only include buses on `route`. Replaces any routes set before.
    pub fn route(self, route: impl Into<Route>) -> Self {
        self.routes([route])
    }

    /// Only include buses on these routes. Duplicates are ignored, and an empty list
    /// means every route. Replaces any routes set before.
    pub fn routes<R: Into<Route>>(mut self, routes: impl IntoIterator<Item = R>) -> Self {
        self.routes = dedup(routes.into_iter().map(Into::into));
        self
    }

    /// Return at most `top` predictions, the soonest first. `0` returns none.
    pub fn top(mut self, top: u32) -> Self {
        self.top = Some(top);
        self
    }

    /// The language for stop names and destinations. See [`Locale`] for what CTA
    /// translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Predictions for buses, built by [`PredictionsQuery::vehicle`] or
/// [`PredictionsQuery::vehicles`]: each bus's upcoming stops.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehiclePredictions {
    vehicles: Vec<VehicleId>,
    top: Option<u32>,
    locale: Option<Locale>,
}

impl VehiclePredictions {
    /// Return at most `top` predictions, the soonest first. `0` returns none.
    pub fn top(mut self, top: u32) -> Self {
        self.top = Some(top);
        self
    }

    /// The language for stop names and destinations. See [`Locale`] for what CTA
    /// translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Which predictions to fetch: buses coming to some stops, or where some buses are going.
///
/// Start from one of the constructors, which return a [`StopPredictions`] or a
/// [`VehiclePredictions`]. Only stop queries can be limited to routes.
///
/// ```
/// use cta::buses::{PredictionsQuery, StopId, VehicleId};
///
/// // The next 5 route 20 buses at Madison & Jefferson
/// let stop = PredictionsQuery::at(StopId(456)).route("20").top(5);
///
/// // Every upcoming stop for two buses
/// let buses = PredictionsQuery::vehicles([VehicleId(8932), VehicleId(7962)])?;
/// # Ok::<(), cta::buses::QueryError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PredictionsQuery {
    /// Buses coming to up to 10 stops.
    Stops(StopPredictions),
    /// Upcoming stops for up to 10 buses.
    Vehicles(VehiclePredictions),
}

impl PredictionsQuery {
    /// The most stop IDs one query can cover.
    pub const MAX_STOPS: usize = 10;
    /// The most vehicle IDs one query can cover.
    pub const MAX_VEHICLES: usize = 10;

    fn for_stops(stops: Vec<StopId>) -> StopPredictions {
        StopPredictions {
            stops,
            routes: Vec::new(),
            top: None,
            locale: None,
        }
    }

    fn for_vehicles(vehicles: Vec<VehicleId>) -> VehiclePredictions {
        VehiclePredictions {
            vehicles,
            top: None,
            locale: None,
        }
    }

    /// Buses coming to one stop.
    pub fn at(stop: StopId) -> StopPredictions {
        Self::for_stops(vec![stop])
    }

    /// Buses coming to up to 10 stops in one request. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoStops`] if `ids` is empty, or [`QueryError::TooManyStops`] if it
    /// has more than 10 distinct IDs.
    pub fn stops(ids: impl IntoIterator<Item = StopId>) -> Result<StopPredictions, QueryError> {
        let ids = dedup(ids);
        match ids.len() {
            0 => Err(QueryError::NoStops),
            n if n > Self::MAX_STOPS => Err(QueryError::TooManyStops(n)),
            _ => Ok(Self::for_stops(ids)),
        }
    }

    /// Every upcoming stop for one bus.
    pub fn vehicle(vehicle: VehicleId) -> VehiclePredictions {
        Self::for_vehicles(vec![vehicle])
    }

    /// Every upcoming stop for up to 10 buses in one request. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoVehicles`] if `ids` is empty, or [`QueryError::TooManyVehicles`]
    /// if it has more than 10 distinct IDs.
    pub fn vehicles(
        ids: impl IntoIterator<Item = VehicleId>,
    ) -> Result<VehiclePredictions, QueryError> {
        let ids = dedup(ids);
        match ids.len() {
            0 => Err(QueryError::NoVehicles),
            n if n > Self::MAX_VEHICLES => Err(QueryError::TooManyVehicles(n)),
            _ => Ok(Self::for_vehicles(ids)),
        }
    }
}

impl From<StopPredictions> for PredictionsQuery {
    fn from(q: StopPredictions) -> Self {
        PredictionsQuery::Stops(q)
    }
}

impl From<&StopPredictions> for PredictionsQuery {
    fn from(q: &StopPredictions) -> Self {
        PredictionsQuery::Stops(q.clone())
    }
}

impl From<VehiclePredictions> for PredictionsQuery {
    fn from(q: VehiclePredictions) -> Self {
        PredictionsQuery::Vehicles(q)
    }
}

impl From<&VehiclePredictions> for PredictionsQuery {
    fn from(q: &VehiclePredictions) -> Self {
        PredictionsQuery::Vehicles(q.clone())
    }
}

impl From<&PredictionsQuery> for PredictionsQuery {
    fn from(q: &PredictionsQuery) -> Self {
        q.clone()
    }
}

/// Build the request for the predictions in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL). Requests always ask
/// for timestamps to the second.
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn predictions_request(base: &Url, key: &str, query: &PredictionsQuery) -> Request {
    let mut url = base
        .join("getpredictions")
        .expect("base URL must be http(s)");
    let (top, locale) = match query {
        PredictionsQuery::Stops(q) => (q.top, q.locale.as_ref()),
        PredictionsQuery::Vehicles(q) => (q.top, q.locale.as_ref()),
    };
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("key", key);
        match query {
            PredictionsQuery::Stops(q) => {
                pairs.append_pair("stpid", &join(&q.stops));
                if !q.routes.is_empty() {
                    pairs.append_pair("rt", &join(&q.routes));
                }
            }
            PredictionsQuery::Vehicles(q) => {
                pairs.append_pair("vid", &join(&q.vehicles));
            }
        }
        if let Some(top) = top {
            pairs.append_pair("top", &top.to_string());
        }
        pairs.append_pair("tmres", "s");
    }
    append_locale(&mut url, locale);
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a predictions response.
///
/// Predictions come soonest first. Stops with no service and buses that aren't in
/// service are left out, not reported as errors.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_predictions(body: &[u8]) -> Result<Vec<Prediction>, ResponseError> {
    let raw: RawPredictions = parse_envelope(body)?;
    let tz = chicago()?;
    Ok(raw
        .prd
        .into_iter()
        .map(|p| to_prediction(p, &tz))
        .collect::<Result<_, _>>()?)
}

fn to_prediction(p: RawPrediction, tz: &TimeZone) -> Result<Prediction, DecodeError> {
    Ok(Prediction {
        predicted_at: p.tmstmp.to_zoned_datetime(tz.clone())?,
        time: p.prdtm.to_zoned_datetime(tz.clone())?,
        kind: PredictionKind::from_cta(&p.typ),
        stop: StopId(p.stpid),
        stop_name: p.stpnm.trim().to_owned(),
        vehicle: p.vid.map(VehicleId),
        distance_ft: p.dstp,
        route: Route(p.rt),
        route_display: p.rtdd,
        direction: Direction::from(p.rtdir),
        destination: p.des.trim().to_owned(),
        delayed: p.dly.unwrap_or(false),
        dynamic_action: DynamicAction::from_code(p.dynamic.unwrap_or(0)),
        trip_id: p.tatripid,
        original_trip_number: p.origtatripno,
        block_id: p.tablockid,
        zone: p.zone.filter(|z| !z.trim().is_empty()),
        passenger_load: PassengerLoad::from_cta(p.psgld.as_deref().unwrap_or("")),
        scheduled_start: scheduled_start(p.stsd, p.stst, tz)?,
        flag_stop: p.flagstop.map_or(FlagStop::Undefined, FlagStop::from_code),
        countdown: Countdown::from_cta(&p.prdctdn),
        gtfs_sequence: p.gtfsseq,
        nbus: p.nbus.filter(|n| !n.trim().is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const BY_STOP: &[u8] = include_bytes!("../../tests/fixtures/bus_predictions_by_stop.json");
    const BY_VEHICLE: &[u8] =
        include_bytes!("../../tests/fixtures/bus_predictions_by_vehicle.json");
    const MADISON: &[u8] = include_bytes!("../../tests/fixtures/bus_predictions_456.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn query_string(query: impl Into<PredictionsQuery>) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = predictions_request(&base, "k", &query.into()).url;
        assert_eq!(url.path(), "/bustime/api/v3/getpredictions");
        url.query().unwrap().to_owned()
    }

    fn chicago_time(h: i8, mi: i8, s: i8) -> ZonedDateTime {
        civil::date(2026, 11, 22)
            .at(h, mi, s, 0)
            .to_zoned_datetime(chicago().unwrap())
            .unwrap()
    }

    #[test]
    fn stop_requests() {
        assert_eq!(
            query_string(PredictionsQuery::at(StopId(456))),
            "key=k&stpid=456&tmres=s&format=json"
        );
        let q = PredictionsQuery::stops([StopId(456), StopId(1)])
            .unwrap()
            .routes(["20", "X9", "20"])
            .top(5)
            .locale("es");
        assert_eq!(
            query_string(&q),
            "key=k&stpid=456%2C1&rt=20%2CX9&top=5&tmres=s&locale=es&format=json"
        );
        // An empty route list means every route.
        let q = PredictionsQuery::at(StopId(456))
            .route("20")
            .routes(Vec::<Route>::new());
        assert_eq!(query_string(q), "key=k&stpid=456&tmres=s&format=json");
    }

    #[test]
    fn vehicle_requests() {
        assert_eq!(
            query_string(PredictionsQuery::vehicle(VehicleId(8932)).top(0)),
            "key=k&vid=8932&top=0&tmres=s&format=json"
        );
        let q = PredictionsQuery::vehicles([VehicleId(8932), VehicleId(7962)]).unwrap();
        assert_eq!(
            query_string(&q),
            "key=k&vid=8932%2C7962&tmres=s&format=json"
        );
    }

    #[test]
    fn limits() {
        assert_eq!(PredictionsQuery::stops([]), Err(QueryError::NoStops));
        assert!(PredictionsQuery::stops((1..=10).map(StopId)).is_ok());
        assert_eq!(
            PredictionsQuery::stops((1..=11).map(StopId)),
            Err(QueryError::TooManyStops(11))
        );
        assert_eq!(PredictionsQuery::vehicles([]), Err(QueryError::NoVehicles));
        assert_eq!(
            PredictionsQuery::vehicles((1..=11).map(VehicleId)),
            Err(QueryError::TooManyVehicles(11))
        );
        // Route filters have no limit.
        let many = (1..=20).map(|n| n.to_string());
        let _ = PredictionsQuery::at(StopId(456)).routes(many);
    }

    #[test]
    fn parses_canceled_trip() {
        let preds = parse_predictions(MADISON).unwrap();
        assert_eq!(preds.len(), 4); // stop 99999999 is left out
        let p = &preds[0];
        assert_eq!(p.vehicle, None);
        assert_eq!(p.dynamic_action, DynamicAction::Canceled);
        assert!(p.dynamic_action.is_canceled());
        assert!(!p.dynamic_action.is_hidden());
        assert_eq!(p.countdown, Countdown::Minutes(-26));
        assert!(p.time < p.predicted_at);
        assert_eq!(p.distance_ft, 0);
        // Minute timestamps (no tmres=s) still parse.
        assert_eq!(p.predicted_at, chicago_time(10, 13, 0));
    }

    #[test]
    fn parses_normal_prediction() {
        let p = &parse_predictions(MADISON).unwrap()[1];
        assert_eq!(p.predicted_at, chicago_time(10, 13, 0));
        assert_eq!(p.time, chicago_time(10, 17, 0));
        assert_eq!(p.kind, PredictionKind::Arrival);
        assert_eq!(p.stop, StopId(456));
        assert_eq!(p.stop_name, "Madison & Jefferson");
        assert_eq!(p.vehicle, Some(VehicleId(8864)));
        assert_eq!(p.distance_ft, 3052);
        assert_eq!(p.route, Route::from("20"));
        assert_eq!(p.route_display, "20");
        assert_eq!(p.direction, Direction::Westbound);
        assert_eq!(p.destination, "Austin");
        assert!(!p.delayed);
        assert_eq!(p.dynamic_action, DynamicAction::None);
        assert_eq!(p.trip_id, "1W-00021");
        assert_eq!(p.original_trip_number, "280450454");
        assert_eq!(p.block_id, "20 -805");
        assert_eq!(p.zone, None);
        assert_eq!(p.passenger_load, PassengerLoad::Unknown);
        assert_eq!(p.scheduled_start, chicago_time(10, 10, 30));
        assert_eq!(p.flag_stop, FlagStop::Normal);
        assert_eq!(p.countdown, Countdown::Minutes(3));
        assert_eq!((p.gtfs_sequence, p.nbus.as_deref()), (None, None));
    }

    #[test]
    fn parses_live_stop_sample() {
        let preds = parse_predictions(BY_STOP).unwrap();
        assert_eq!(preds.len(), 20);
        let canceled = preds
            .iter()
            .find(|p| p.stop == StopId(11067) && p.vehicle.is_none())
            .unwrap();
        assert_eq!(canceled.dynamic_action, DynamicAction::Canceled);
        assert_eq!(canceled.countdown, Countdown::Minutes(-3));
        assert_eq!(canceled.destination, "Logan Square Blue Line");
        let departure = preds.iter().find(|p| p.stop == StopId(1)).unwrap();
        assert_eq!(departure.kind, PredictionKind::Departure);
        assert_eq!(departure.vehicle, Some(VehicleId(8307)));
        assert_eq!(departure.time, chicago_time(13, 11, 0));
        assert_eq!(departure.predicted_at, chicago_time(12, 53, 9));
    }

    #[test]
    fn parses_live_vehicle_sample() {
        let preds = parse_predictions(BY_VEHICLE).unwrap();
        assert_eq!(preds.len(), 320);
        let p = &preds[0];
        assert_eq!(p.vehicle, Some(VehicleId(1071)));
        assert_eq!(p.countdown, Countdown::Due);
        assert_eq!(p.stop_name, "Belmont & Pacific (80th Avenue)");
        assert!(preds.iter().all(|p| p.vehicle.is_some()));
    }

    #[test]
    fn eta_is_measured_from_now() {
        let p = &parse_predictions(MADISON).unwrap()[1]; // due 10:17:00
        assert_eq!(
            p.eta_from(&chicago_time(10, 15, 30)),
            SignedDuration::from_secs(90)
        );
        assert_eq!(
            p.eta_from(&chicago_time(10, 17, 30)),
            SignedDuration::from_secs(-30)
        );
    }

    #[test]
    fn dynamic_action_codes() {
        let documented = [0, 1, 2, 3, 4, 6, 8, 9, 10, 12, 13, 14, 15, 16, 17, 18, 19];
        for code in documented {
            assert!(
                !matches!(DynamicAction::from_code(code), DynamicAction::Other(_)),
                "{code}"
            );
        }
        for code in [5, 7, 11, 20, 255] {
            assert_eq!(DynamicAction::from_code(code), DynamicAction::Other(code));
        }
        let canceled: Vec<_> = (0..=20)
            .filter(|&c| DynamicAction::from_code(c).is_canceled())
            .collect();
        assert_eq!(canceled, [1, 12, 18]);
        let hidden: Vec<_> = (0..=20)
            .filter(|&c| DynamicAction::from_code(c).is_hidden())
            .collect();
        assert_eq!(hidden, [12, 16, 17]);
        let drop_off: Vec<_> = (0..=20)
            .filter(|&c| DynamicAction::from_code(c).is_drop_off_only())
            .collect();
        assert_eq!(drop_off, [4]);
    }

    #[test]
    fn kinds_flag_stops_and_countdowns() {
        assert_eq!(PredictionKind::from_cta("A"), PredictionKind::Arrival);
        assert_eq!(PredictionKind::from_cta("d"), PredictionKind::Departure);
        assert_eq!(
            PredictionKind::from_cta("X"),
            PredictionKind::Other("X".into())
        );
        assert_eq!(FlagStop::from_code(-1), FlagStop::Undefined);
        assert_eq!(FlagStop::from_code(0), FlagStop::Normal);
        assert_eq!(FlagStop::from_code(1), FlagStop::PickupAndDischarge);
        assert_eq!(FlagStop::from_code(2), FlagStop::DischargeOnly);
        assert_eq!(FlagStop::from_code(7), FlagStop::Other(7));
        assert_eq!(Countdown::from_cta("DUE"), Countdown::Due);
        assert_eq!(Countdown::from_cta("DLY"), Countdown::Delayed);
        assert_eq!(Countdown::from_cta("12"), Countdown::Minutes(12));
        assert_eq!(Countdown::from_cta("-3"), Countdown::Minutes(-3));
        assert_eq!(Countdown::from_cta("SOON"), Countdown::Other("SOON".into()));
    }

    #[test]
    fn parses_delayed_and_documented_fields_cta_does_not_send() {
        let body = br#"{"bustime-response": {"prd": [{
            "tmstmp": "20261122 12:50:46", "typ": "A", "stpnm": " Clark & Lake ", "stpid": "1",
            "vid": "8152", "dstp": 100, "rt": "20", "rtdd": "20", "rtdir": "Eastbound",
            "des": "Michigan", "prdtm": "20261122 12:57:46", "tablockid": "b",
            "tatripid": "t", "origtatripno": "o", "dly": true, "dyn": 16, "prdctdn": "DLY",
            "zone": "Bay 1", "psgld": "FULL", "stst": 43200, "stsd": "2026-11-22",
            "flagstop": 2, "gtfsseq": "14", "nbus": "x"
        }]}}"#;
        let p = parse_predictions(body).unwrap().remove(0);
        assert_eq!(p.stop_name, "Clark & Lake");
        assert!(p.delayed);
        assert_eq!(p.countdown, Countdown::Delayed);
        assert_eq!(p.dynamic_action, DynamicAction::InvalidatedTrip);
        assert!(p.dynamic_action.is_hidden());
        assert_eq!(p.zone.as_deref(), Some("Bay 1"));
        assert_eq!(p.passenger_load, PassengerLoad::Full);
        assert_eq!(p.flag_stop, FlagStop::DischargeOnly);
        assert_eq!(p.gtfs_sequence, Some(14));
        assert_eq!(p.nbus.as_deref(), Some("x"));
    }

    #[test]
    fn missing_flag_stop_and_dyn_have_defaults() {
        let body = br#"{"bustime-response": {"prd": [{
            "tmstmp": "20261122 12:50:46", "typ": "A", "stpnm": "A", "stpid": "1",
            "vid": "1", "dstp": 0, "rt": "20", "rtdd": "20", "rtdir": "Eastbound",
            "des": "Michigan", "prdtm": "20261122 12:57:46", "tablockid": "b",
            "tatripid": "t", "origtatripno": "o", "prdctdn": "7",
            "stst": 43200, "stsd": "2026-11-22"
        }]}}"#;
        let p = parse_predictions(body).unwrap().remove(0);
        assert_eq!(p.flag_stop, FlagStop::Undefined);
        assert_eq!(p.dynamic_action, DynamicAction::None);
        assert!(!p.delayed);
    }

    #[test]
    fn request_errors_are_api_errors() {
        // What hand-built requests get; the query types never send these.
        for msg in [
            "Maximum number of stpid identifiers exceeded",
            "No parameter provided",
            "Invalid top parameter provided",
        ] {
            let body = format!(r#"{{"bustime-response": {{"error": [{{"msg": "{msg}"}}]}}}}"#);
            assert!(matches!(
                parse_predictions(body.as_bytes()),
                Err(ResponseError::Api(e)) if e.message == msg
            ));
        }
        assert!(matches!(
            parse_predictions(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[test]
    fn top_zero_response_is_empty() {
        assert!(
            parse_predictions(br#"{"bustime-response": {}}"#)
                .unwrap()
                .is_empty()
        );
    }
}
