use super::{Position, Request, Route, StationId, StopId, parse_envelope, position};
use crate::deserializers::{flag, from_str, one_or_many, opt_from_str};
use crate::{DecodeError, ResponseError, chicago};
use jiff::{ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawLocations {
    #[serde(default, deserialize_with = "one_or_many")]
    route: Vec<RawRoute>,
}

#[derive(Deserialize)]
struct RawRoute {
    #[serde(rename = "@name")]
    name: String,
    #[serde(default, deserialize_with = "one_or_many")]
    train: Vec<RawTrain>,
}

#[derive(Deserialize)]
struct RawTrain {
    rn: String,
    #[serde(rename = "destSt", deserialize_with = "from_str")]
    dest_st: u32,
    #[serde(rename = "destNm")]
    dest_nm: String,
    #[serde(rename = "trDr", deserialize_with = "from_str")]
    tr_dr: u8,
    #[serde(rename = "nextStaId", deserialize_with = "from_str")]
    next_sta_id: u32,
    #[serde(rename = "nextStpId", deserialize_with = "from_str")]
    next_stp_id: u32,
    #[serde(rename = "nextStaNm")]
    next_sta_nm: String,
    #[serde(deserialize_with = "from_str")]
    prdt: civil::DateTime,
    #[serde(rename = "arrT", deserialize_with = "from_str")]
    arr_t: civil::DateTime,
    #[serde(rename = "isApp", deserialize_with = "flag")]
    is_app: bool,
    #[serde(rename = "isDly", deserialize_with = "flag")]
    is_dly: bool,
    #[serde(default, deserialize_with = "opt_from_str")]
    lat: Option<f64>,
    #[serde(default, deserialize_with = "opt_from_str")]
    lon: Option<f64>,
    #[serde(default, deserialize_with = "opt_from_str")]
    heading: Option<u16>,
}

/// A train currently in service, and the station it's headed to next.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct TrainLocation {
    /// The line the train is running on.
    pub route: Route,
    /// The train's run number. Pass it to `follow` to see all its upcoming stops.
    pub run: String,
    /// The stop at the end of the train's run.
    pub destination_stop: StopId,
    /// Where the train is headed, e.g. `"Howard"`.
    ///
    /// Occasionally a placeholder such as `"See train"`.
    pub destination: String,
    /// The CTA's direction code, `1` or `5`. Its meaning depends on the line.
    pub direction: u8,
    /// The next station the train will reach.
    pub next_station_id: StationId,
    /// The platform at the next station.
    pub next_stop_id: StopId,
    /// The next station's name.
    pub next_station: String,
    /// When the CTA made this prediction.
    pub predicted_at: ZonedDateTime,
    /// When the train is predicted to reach the next station.
    pub arrival_time: ZonedDateTime,
    /// The train is approaching or at the next station.
    pub approaching: bool,
    /// The train is delayed.
    pub delayed: bool,
    /// The train's last reported position, if known.
    pub position: Option<Position>,
}

/// Which lines to fetch train locations for.
///
/// ```
/// use cta::trains::{LocationsQuery, Route};
///
/// let red = LocationsQuery::route(Route::Red);
/// let loop_lines = LocationsQuery::routes([Route::Brown, Route::Orange, Route::Pink]);
/// let everything = LocationsQuery::all();
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocationsQuery {
    routes: Vec<Route>,
}

impl LocationsQuery {
    /// Trains on one line.
    pub fn route(route: Route) -> Self {
        Self {
            routes: vec![route],
        }
    }

    /// Trains on several lines in one request. Duplicates are ignored.
    ///
    /// An empty list is allowed and returns no trains.
    pub fn routes(routes: impl IntoIterator<Item = Route>) -> Self {
        let mut v: Vec<Route> = Vec::new();
        for r in routes {
            if !v.contains(&r) {
                v.push(r);
            }
        }
        Self { routes: v }
    }

    /// Every line.
    pub fn all() -> Self {
        Self::routes(Route::ALL)
    }
}

/// Build the request for a locations query, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn locations_request(base: &Url, key: &str, query: &LocationsQuery) -> Request {
    let mut url = base
        .join("ttpositions.aspx")
        .expect("base URL must be http(s)");
    let rt = query
        .routes
        .iter()
        .map(|r| r.code())
        .collect::<Vec<_>>()
        .join(",");
    url.query_pairs_mut()
        .append_pair("key", key)
        .append_pair("rt", &rt)
        .append_pair("outputType", "JSON");
    Request { url }
}

/// Parse the body of a locations response into one list of trains across all lines.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error code, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_locations(body: &[u8]) -> Result<Vec<TrainLocation>, ResponseError> {
    let raw: RawLocations = parse_envelope(body)?;
    let tz = chicago()?;

    let mut out = Vec::new();
    for r in raw.route {
        let route: Route = r.name.parse().map_err(DecodeError::from)?;
        for t in r.train {
            out.push(to_train_location(route, t, &tz)?);
        }
    }
    Ok(out)
}

fn to_train_location(
    route: Route,
    t: RawTrain,
    tz: &TimeZone,
) -> Result<TrainLocation, DecodeError> {
    Ok(TrainLocation {
        route,
        run: t.rn,
        destination_stop: StopId(t.dest_st),
        destination: t.dest_nm,
        direction: t.tr_dr,
        next_station_id: StationId(t.next_sta_id),
        next_stop_id: StopId(t.next_stp_id),
        next_station: t.next_sta_nm,
        predicted_at: t.prdt.to_zoned_datetime(tz.clone())?,
        arrival_time: t.arr_t.to_zoned_datetime(tz.clone())?,
        approaching: t.is_app,
        delayed: t.is_dly,
        position: position(t.lat, t.lon, t.heading),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trains::DEFAULT_BASE_URL;

    #[test]
    fn parses_red_blue_fixture() {
        let body = include_bytes!("../../tests/fixtures/locations_red_blue.json");
        let trains = parse_locations(body).unwrap();
        assert_eq!(trains.len(), 4);

        let t = &trains[0];
        assert_eq!(t.route, Route::Red); // from lowercase "@name":"red"
        assert_eq!(t.run, "826");
        assert_eq!(t.destination_stop, StopId(30089));
        assert_eq!(t.direction, 5);
        assert_eq!(t.next_station_id, StationId(41170));
        assert_eq!(t.next_stop_id, StopId(30224));
        assert_eq!(t.next_station, "Garfield");
        assert!(t.approaching && !t.delayed);
        assert_eq!(t.position.unwrap().heading, 178);

        assert_eq!(trains[2].route, Route::Blue);
        assert_eq!(trains[3].destination, "See train");
    }

    #[test]
    fn parses_yellow_fixture() {
        let body = include_bytes!("../../tests/fixtures/locations_yellow.json");
        let trains = parse_locations(body).unwrap();
        assert_eq!(trains.len(), 1);
        assert_eq!(trains[0].route, Route::Yellow);
        assert_eq!(
            trains[0].arrival_time.to_string(),
            "2026-11-19T18:46:19-06:00[America/Chicago]"
        );
    }

    #[test]
    fn single_route_and_train_objects() {
        let train = r#"{"rn":"595","destSt":"30026","destNm":"Skokie","trDr":"1","nextStaId":"41680","nextStpId":"30297","nextStaNm":"Oakton-Skokie","prdt":"2026-11-19T18:45:19","arrT":"2026-11-19T18:46:19","isApp":"1","isDly":"0","flags":null,"lat":"42.02363","lon":"-87.74511","heading":"122"}"#;
        let body = format!(
            r#"{{"ctatt":{{"errCd":"0","errNm":null,"route":{{"@name":"y","train":{train}}}}}}}"#
        );
        let trains = parse_locations(body.as_bytes()).unwrap();
        assert_eq!(trains.len(), 1);
        assert_eq!(trains[0].route, Route::Yellow);
    }

    #[test]
    fn route_with_no_trains_is_empty() {
        let body = br#"{"ctatt":{"errCd":"0","errNm":null,"route":[{"@name":"y"}]}}"#;
        assert!(parse_locations(body).unwrap().is_empty());
    }

    #[test]
    fn empty_query_response_is_empty() {
        // Real response to `rt=` with no routes.
        let body = br#"{"ctatt":{"tmst":"2026-11-20T14:40:19","errCd":"0","errNm":null}}"#;
        assert!(parse_locations(body).unwrap().is_empty());
    }

    #[test]
    fn request_joins_codes_and_dedupes() {
        let base = Url::parse(DEFAULT_BASE_URL).unwrap();
        let q = LocationsQuery::routes([Route::Red, Route::Brown, Route::Red]);
        let req = locations_request(&base, "k", &q);
        assert_eq!(req.url.path(), "/api/1.0/ttpositions.aspx");
        assert_eq!(req.url.query(), Some("key=k&rt=Red%2CBrn&outputType=JSON"));
    }

    #[test]
    fn all_covers_every_route() {
        assert_eq!(LocationsQuery::all(), LocationsQuery::routes(Route::ALL));
        let base = Url::parse(DEFAULT_BASE_URL).unwrap();
        let req = locations_request(&base, "k", &LocationsQuery::all());
        let rt = req.url.query_pairs().find(|(k, _)| k == "rt").unwrap().1;
        assert_eq!(rt.split(',').count(), Route::ALL.len());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn locations_round_trip_through_json() {
        let trains = parse_locations(include_bytes!(
            "../../tests/fixtures/locations_red_blue.json"
        ))
        .unwrap();
        let json = serde_json::to_string(&trains).unwrap();
        let back: Vec<TrainLocation> = serde_json::from_str(&json).unwrap();
        assert_eq!(trains, back);
    }
}
