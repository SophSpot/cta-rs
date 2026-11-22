use super::{QueryError, Service, normalize_color, parse_envelope};
use crate::deserializers::{one_or_many, opt_cdata};
use crate::trains::{self, StationId};
use crate::{Request, ResponseError, buses};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawRoutes {
    #[serde(default, rename = "RouteInfo", deserialize_with = "one_or_many")]
    route_info: Vec<RawRouteInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawRouteInfo {
    route: String,
    #[serde(default)]
    route_color_code: String,
    #[serde(default)]
    route_text_color: String,
    service_id: String,
    #[serde(default, rename = "RouteURL", deserialize_with = "opt_cdata")]
    route_url: Option<String>,
    #[serde(default)]
    route_status: String,
    #[serde(default)]
    route_status_color: String,
}

/// The current status of a route, station, or systemwide group, as returned by
/// [`Alerts::route_status`](crate::client::Alerts::route_status).
///
/// The status reflects the most severe active alert affecting the service.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct RouteStatus {
    /// What this status is for.
    pub service: Service,
    /// The service's name, e.g. `"Red Line"`, `"Madison"`, `"Clark/Lake"`, or
    /// `"All Bus Routes"`.
    pub name: String,
    /// For a station, its street address, e.g. `"124 W. Lake St., Chicago, IL 60601"`.
    pub address: Option<String>,
    /// For a station, the line this entry is about.
    ///
    /// A station appears once for each line serving it, each with that line's status,
    /// and Customer Alerts identifies the line only by [`color`](RouteStatus::color).
    /// This is inferred from that color, and is `None` if it isn't a known line color,
    /// or for entries other than stations.
    pub line: Option<trains::Route>,
    /// The route's color, as `"#rrggbb"`.
    pub color: String,
    /// A text color that reads well on [`color`](RouteStatus::color), as `"#rrggbb"`.
    pub text_color: String,
    /// The service's page on transitchicago.com.
    pub url: Option<String>,
    /// The status, e.g. `"Normal Service"`, `"Planned Reroute"`, or
    /// `"Minor Delays / Reroute"`.
    pub status: String,
    /// The color CTA uses for this status, as `"#rrggbb"`.
    pub status_color: String,
}

impl RouteStatus {
    /// The status is `"Normal Service"`.
    pub fn is_normal(&self) -> bool {
        self.status.eq_ignore_ascii_case("Normal Service")
    }
}

/// A kind of service whose statuses can be listed, for [`RouteStatusQuery::types`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ServiceKind {
    /// Bus routes.
    Bus,
    /// Train ('L') routes.
    Rail,
    /// Train stations, one entry for each line serving each station.
    Station,
    /// The systemwide groups: all routes, all bus routes, all train routes.
    Systemwide,
}

impl ServiceKind {
    fn as_str(self) -> &'static str {
        match self {
            ServiceKind::Bus => "bus",
            ServiceKind::Rail => "rail",
            ServiceKind::Station => "station",
            ServiceKind::Systemwide => "systemwide",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RouteStatusTarget {
    All,
    Types(Vec<ServiceKind>),
    TrainRoutes(Vec<trains::Route>),
    BusRoutes(Vec<buses::Route>),
    Station(StationId),
}

/// Which statuses to list: the default set, some kinds of service, some routes, or one
/// station.
///
/// Train and bus routes are separate constructors because Customer Alerts silently
/// drops the train routes from a request that mixes the two.
///
/// ```
/// use cta::alerts::{RouteStatusQuery, ServiceKind};
/// use cta::trains::{Route, StationId};
///
/// let everything = RouteStatusQuery::all();
/// let stations = RouteStatusQuery::types([ServiceKind::Station])?;
/// let lines = RouteStatusQuery::train_routes([Route::Red, Route::Blue])?;
/// let buses = RouteStatusQuery::bus_routes(["20", "X9"])?;
/// let clark_lake = RouteStatusQuery::station(StationId(40380));
/// # Ok::<(), cta::alerts::QueryError>(())
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteStatusQuery {
    target: RouteStatusTarget,
}

impl RouteStatusQuery {
    /// Every bus route, train route, and systemwide group (Customer Alerts' default).
    /// Stations aren't included; use [`types`](Self::types) for those.
    pub fn all() -> Self {
        Self {
            target: RouteStatusTarget::All,
        }
    }

    /// Every service of these kinds. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoTypes`] if `kinds` is empty.
    pub fn types(kinds: impl IntoIterator<Item = ServiceKind>) -> Result<Self, QueryError> {
        let kinds = super::dedup(kinds);
        if kinds.is_empty() {
            return Err(QueryError::NoTypes);
        }
        Ok(Self {
            target: RouteStatusTarget::Types(kinds),
        })
    }

    /// These train lines. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoRoutes`] if `routes` is empty.
    pub fn train_routes(
        routes: impl IntoIterator<Item = trains::Route>,
    ) -> Result<Self, QueryError> {
        let routes = super::dedup(routes);
        if routes.is_empty() {
            return Err(QueryError::NoRoutes);
        }
        Ok(Self {
            target: RouteStatusTarget::TrainRoutes(routes),
        })
    }

    /// These bus routes. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoRoutes`] if `routes` is empty.
    pub fn bus_routes<R: Into<buses::Route>>(
        routes: impl IntoIterator<Item = R>,
    ) -> Result<Self, QueryError> {
        let routes = super::dedup(routes.into_iter().map(Into::into));
        if routes.is_empty() {
            return Err(QueryError::NoRoutes);
        }
        Ok(Self {
            target: RouteStatusTarget::BusRoutes(routes),
        })
    }

    /// One train station, with one entry for each line serving it. Customer Alerts
    /// accepts only one station per request.
    pub fn station(station: StationId) -> Self {
        Self {
            target: RouteStatusTarget::Station(station),
        }
    }
}

/// Build the request for the statuses in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn route_status_request(base: &Url, query: &RouteStatusQuery) -> Request {
    let mut url = base.join("routes.aspx").expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        match &query.target {
            RouteStatusTarget::All => {}
            RouteStatusTarget::Types(kinds) => {
                let kinds: Vec<_> = kinds.iter().map(|k| k.as_str()).collect();
                q.append_pair("type", &kinds.join(","));
            }
            RouteStatusTarget::TrainRoutes(routes) => {
                let codes: Vec<_> = routes.iter().map(|r| r.code()).collect();
                q.append_pair("routeid", &codes.join(","));
            }
            RouteStatusTarget::BusRoutes(routes) => {
                let codes: Vec<_> = routes.iter().map(buses::Route::as_str).collect();
                q.append_pair("routeid", &codes.join(","));
            }
            RouteStatusTarget::Station(station) => {
                q.append_pair("stationid", &station.to_string());
            }
        }
        q.append_pair("outputType", "JSON");
    }
    Request { url }
}

/// Parse the body of a route status response.
///
/// Routes and stations that don't exist are left out rather than reported as errors.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, or [`ResponseError::Decode`] if
/// the body couldn't be read.
pub fn parse_route_status(body: &[u8]) -> Result<Vec<RouteStatus>, ResponseError> {
    let raw: RawRoutes = parse_envelope(body, "CTARoutes")?;
    Ok(raw.route_info.into_iter().map(to_route_status).collect())
}

fn to_route_status(r: RawRouteInfo) -> RouteStatus {
    let service = Service::classify(&r.service_id);
    let (name, address) = match (&service, r.route.split_once(" | ")) {
        (Service::Station(_), Some((name, address))) => {
            (name.trim().to_owned(), Some(address.trim().to_owned()))
        }
        _ => (r.route.trim().to_owned(), None),
    };
    let color = normalize_color(&r.route_color_code);
    let line = match service {
        Service::Station(_) => line_for_color(&color),
        _ => None,
    };
    RouteStatus {
        service,
        name,
        address,
        line,
        color,
        text_color: normalize_color(&r.route_text_color),
        url: r.route_url,
        status: r.route_status.trim().to_owned(),
        status_color: normalize_color(&r.route_status_color),
    }
}

/// The 'L' line with this color, from Customer Alerts' own train route entries.
fn line_for_color(color: &str) -> Option<trains::Route> {
    use trains::Route::*;
    Some(match color {
        "#c60c30" => Red,
        "#00a1de" => Blue,
        "#62361b" => Brown,
        "#009b3a" => Green,
        "#f9461c" => Orange,
        "#e27ea6" => Pink,
        "#522398" => Purple,
        "#f9e300" => Yellow,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::Systemwide;
    use crate::{Api, ApiErrorKind};

    const ALL: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes.json");
    const STATIONS: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes_stations.json");
    const CLARK_LAKE: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes_clark_lake.json");
    const SYSTEMWIDE: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes_systemwide.json");
    const NONE: &[u8] = include_bytes!("../../tests/fixtures/alerts_routes_no_results.json");
    const BAD_PARAM: &[u8] = include_bytes!("../../tests/fixtures/alerts_error_500.json");

    fn query_string(query: &RouteStatusQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = route_status_request(&base, query).url;
        assert_eq!(
            url.as_str().split('?').next(),
            Some("https://www.transitchicago.com/api/1.0/routes.aspx")
        );
        url.query().unwrap().to_owned()
    }

    #[test]
    fn requests() {
        assert_eq!(query_string(&RouteStatusQuery::all()), "outputType=JSON");
        let q =
            RouteStatusQuery::types([ServiceKind::Rail, ServiceKind::Station, ServiceKind::Rail])
                .unwrap();
        assert_eq!(query_string(&q), "type=rail%2Cstation&outputType=JSON");
        let q = RouteStatusQuery::train_routes([trains::Route::Red, trains::Route::Brown]).unwrap();
        assert_eq!(query_string(&q), "routeid=Red%2CBrn&outputType=JSON");
        let q = RouteStatusQuery::bus_routes(["20", "X9", "20"]).unwrap();
        assert_eq!(query_string(&q), "routeid=20%2CX9&outputType=JSON");
        assert_eq!(
            query_string(&RouteStatusQuery::station(StationId(40380))),
            "stationid=40380&outputType=JSON"
        );
    }

    #[test]
    fn empty_lists_are_errors() {
        assert_eq!(RouteStatusQuery::types([]), Err(QueryError::NoTypes));
        assert_eq!(
            RouteStatusQuery::train_routes([]),
            Err(QueryError::NoRoutes)
        );
        assert_eq!(
            RouteStatusQuery::bus_routes(Vec::<buses::Route>::new()),
            Err(QueryError::NoRoutes)
        );
    }

    #[test]
    fn parses_default_list() {
        let all = parse_route_status(ALL).unwrap();
        assert_eq!(all.len(), 138);
        let count = |f: fn(&Service) -> bool| all.iter().filter(|s| f(&s.service)).count();
        assert_eq!(count(|s| matches!(s, Service::BusRoute(_))), 127);
        assert_eq!(count(|s| matches!(s, Service::TrainRoute(_))), 8);
        assert_eq!(count(|s| matches!(s, Service::Systemwide(_))), 3);
        assert_eq!(count(|s| matches!(s, Service::Other(_))), 0);

        let red = &all[0];
        assert_eq!(red.service, Service::TrainRoute(trains::Route::Red));
        assert_eq!(red.name, "Red Line");
        assert_eq!(red.address, None);
        assert_eq!(red.line, None);
        assert_eq!(red.color, "#c60c30");
        assert_eq!(red.text_color, "#ffffff");
        assert_eq!(
            red.url.as_deref(),
            Some("http://www.transitchicago.com/redline/")
        );
        assert_eq!(red.status_color, "#404040");
        assert!(red.is_normal());

        let madison = all
            .iter()
            .find(|s| s.service == Service::BusRoute("20".into()))
            .unwrap();
        assert_eq!(madison.name, "Madison");
    }

    #[test]
    fn parses_systemwide_groups() {
        let groups: Vec<_> = parse_route_status(SYSTEMWIDE)
            .unwrap()
            .into_iter()
            .map(|s| (s.service, s.name))
            .collect();
        assert_eq!(
            groups,
            [
                (
                    Service::Systemwide(Systemwide::AllRoutes),
                    "All Routes".to_owned()
                ),
                (
                    Service::Systemwide(Systemwide::AllBusRoutes),
                    "All Bus Routes".to_owned()
                ),
                (
                    Service::Systemwide(Systemwide::AllTrainRoutes),
                    "All Train Routes".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn every_station_gets_a_name_address_and_line() {
        let stations = parse_route_status(STATIONS).unwrap();
        assert_eq!(stations.len(), 191);
        for s in &stations {
            assert!(matches!(s.service, Service::Station(_)), "{s:?}");
            assert!(!s.name.contains('|') && !s.name.is_empty(), "{s:?}");
            assert!(
                // Most end ", Chicago, IL 606xx", but not all: Logan Square has no "IL".
                s.address.as_deref().is_some_and(|a| !a.is_empty()),
                "{s:?}"
            );
            assert!(s.line.is_some(), "no line for {s:?}");
        }
    }

    #[test]
    fn station_has_one_entry_per_line() {
        let entries = parse_route_status(CLARK_LAKE).unwrap();
        let mut lines: Vec<_> = entries.iter().map(|s| s.line.unwrap().name()).collect();
        lines.sort();
        assert_eq!(
            lines,
            ["Blue", "Brown", "Green", "Orange", "Pink", "Purple"]
        );
        for s in &entries {
            assert_eq!(s.service, Service::Station(StationId(40380)));
            assert_eq!(s.name, "Clark/Lake");
            assert_eq!(
                s.address.as_deref(),
                Some("124 W. Lake St., Chicago, IL 60601")
            );
        }
        // Only the Green Line platform had planned work in this response.
        let green = entries
            .iter()
            .find(|s| s.line == Some(trains::Route::Green))
            .unwrap();
        assert_eq!(green.status, "Planned Work");
        assert_eq!(green.status_color, "#0066cc"); // sent as "06c"
        assert!(!green.is_normal());
        assert!(
            entries
                .iter()
                .filter(|s| s.line != Some(trains::Route::Green))
                .all(RouteStatus::is_normal)
        );
    }

    #[test]
    fn unknown_color_has_no_line() {
        assert_eq!(line_for_color("#123456"), None);
        assert_eq!(line_for_color("#c60c30"), Some(trains::Route::Red));
    }

    #[test]
    fn no_results_is_empty_and_bad_param_is_error() {
        assert!(parse_route_status(NONE).unwrap().is_empty());
        match parse_route_status(BAD_PARAM) {
            Err(ResponseError::Api(e)) => {
                assert_eq!((e.api, e.code), (Api::Alerts, Some(500)));
                assert_eq!(e.kind(), ApiErrorKind::Other);
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }
}
