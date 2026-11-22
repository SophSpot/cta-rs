use super::{QueryError, Service, Systemwide, dedup, normalize_color, parse_envelope};
use crate::deserializers::{num_or_str, one_or_many, opt_cdata, opt_num_or_str};
use crate::trains::{self, StationId};
use crate::{DecodeError, Request, ResponseError, buses, chicago};
use jiff::{ZonedDateTime, civil, tz::TimeZone};
use serde::Deserialize;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use url::Url;

#[derive(Deserialize)]
struct RawAlerts {
    #[serde(default, rename = "Alert", deserialize_with = "one_or_many")]
    alert: Vec<RawAlert>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawAlert {
    #[serde(deserialize_with = "num_or_str")]
    alert_id: u32,
    #[serde(default, rename = "GUID")]
    guid: Option<String>,
    #[serde(default)]
    headline: String,
    #[serde(default)]
    short_description: String,
    #[serde(default, deserialize_with = "opt_cdata")]
    full_description: Option<String>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    severity_score: Option<u8>,
    #[serde(default)]
    severity_color: String,
    #[serde(default, rename = "SeverityCSS")]
    severity_css: String,
    #[serde(default)]
    impact: String,
    event_start: String,
    #[serde(default)]
    event_end: Option<String>,
    #[serde(default, deserialize_with = "opt_num_or_str")]
    major_alert: Option<u8>,
    #[serde(default, rename = "AlertURL", deserialize_with = "opt_cdata")]
    alert_url: Option<String>,
    #[serde(default)]
    impacted_service: Option<RawImpactedService>,
    #[serde(default, rename = "ttim", deserialize_with = "opt_num_or_str")]
    ttim: Option<u8>,
}

#[derive(Deserialize)]
struct RawImpactedService {
    #[serde(default, rename = "Service", deserialize_with = "one_or_many")]
    service: Vec<RawService>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawService {
    #[serde(default)]
    service_type: String,
    #[serde(default)]
    service_name: String,
    #[serde(default)]
    service_id: String,
    #[serde(default)]
    service_back_color: String,
    #[serde(default)]
    service_text_color: String,
    #[serde(default, rename = "ServiceURL", deserialize_with = "opt_cdata")]
    service_url: Option<String>,
}

/// An alert's ID, e.g. `93306`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlertId(pub u32);

impl Display for AlertId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

/// A service alert, as returned by [`Alerts::detailed`](crate::client::Alerts::detailed).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Alert {
    /// The alert's ID.
    pub id: AlertId,
    /// Customer Alerts' `GUID` field. Undocumented, but apparently a unique ID for the
    /// alert.
    pub guid: Option<String>,
    /// A headline, e.g. `"Temporary Bus Stop Change"`.
    pub headline: String,
    /// A one- or two-sentence description.
    pub short_description: String,
    /// The full description, as HTML from transitchicago.com (paragraphs, bold text,
    /// images, and entities such as `&nbsp;`).
    pub full_description: String,
    /// How the alert affects service, e.g. `"Planned Reroute"` or `"Elevator Status"`.
    pub impact: String,
    /// How severe the alert is.
    pub severity: Severity,
    /// When the alert starts.
    pub start: AlertTime,
    /// When the alert ends, or `None` if it's open-ended.
    ///
    /// A date without a time probably means the alert runs through the end of that day;
    /// Customer Alerts doesn't say.
    pub end: Option<AlertTime>,
    /// CTA marks this as a major alert, shown in a red banner across transitchicago.com.
    pub major: bool,
    /// The alert's page on transitchicago.com.
    pub url: Option<String>,
    /// The routes, stations, or systemwide groups the alert affects.
    pub services: Vec<AffectedService>,
    /// Customer Alerts' `ttim` field. Undocumented; its meaning is unknown.
    pub ttim: bool,
}

/// A route, station, or systemwide group affected by an [`Alert`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct AffectedService {
    /// The service.
    pub service: Service,
    /// The service's name, e.g. `"Clark"`, `"Green Line"`, `"Clark/Lake"`, or
    /// `"All Bus Routes"`.
    pub name: String,
    /// The service's color, as `"#rrggbb"`.
    pub color: String,
    /// A text color that reads well on [`color`](AffectedService::color), as `"#rrggbb"`.
    pub text_color: String,
    /// The service's page on transitchicago.com.
    pub url: Option<String>,
}

/// How severe an [`Alert`] is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct Severity {
    /// A score from 0 to 99; higher is more severe. Planned events and those affecting
    /// only some riders score lower than unplanned ones. See [`band`](Severity::band)
    /// for how CTA groups scores.
    pub score: u8,
    /// The category CTA uses to pick an alert's icon and style.
    pub category: SeverityCategory,
    /// The color CTA uses for the alert's text, as `"#rrggbb"`.
    pub color: String,
}

impl Severity {
    /// The group [`score`](Severity::score) falls in, per the Customer Alerts guide.
    ///
    /// The guide notes these groups are subject to change, and they don't always agree
    /// with [`category`](Severity::category): a `Planned` alert can score in the
    /// `Informational` band.
    pub fn band(&self) -> SeverityBand {
        match self.score {
            0..=19 => SeverityBand::Informational,
            20..=39 => SeverityBand::Planned,
            40..=59 => SeverityBand::MinorDelays,
            60..=79 => SeverityBand::SignificantDelays,
            _ => SeverityBand::MajorDisruption,
        }
    }
}

/// The category of an alert's [`Severity`] (Customer Alerts' `SeverityCSS`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SeverityCategory {
    /// `normal`.
    Normal,
    /// `planned`.
    Planned,
    /// `minor`.
    Minor,
    /// `major`.
    Major,
    /// `special-note`, used for notices such as elevator outages. Not in the guide.
    SpecialNote,
    /// A category other than the above, as Customer Alerts sent it.
    Other(String),
}

impl SeverityCategory {
    fn from_cta(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "normal" => SeverityCategory::Normal,
            "planned" => SeverityCategory::Planned,
            "minor" => SeverityCategory::Minor,
            "major" => SeverityCategory::Major,
            "special-note" => SeverityCategory::SpecialNote,
            _ => SeverityCategory::Other(s.trim().to_owned()),
        }
    }
}

/// How CTA groups [`Severity::score`]s, from [`Severity::band`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum SeverityBand {
    /// 0–19: accessibility and informational alerts.
    Informational,
    /// 20–39: planned or anticipated events, such as planned work or parade reroutes.
    Planned,
    /// 40–59: unanticipated minor delays and reroutes.
    MinorDelays,
    /// 60–79: unanticipated significant delays and reroutes.
    SignificantDelays,
    /// 80–99: major delays and disruptions, where alternatives may be advisable.
    MajorDisruption,
}

/// When an alert starts or ends: a day, or an exact time if Customer Alerts gives one.
///
/// Times sort by day, then a day without a time before any exact time that day.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum AlertTime {
    /// A day, with no time given.
    Date(civil::Date),
    /// An exact time, in Chicago.
    At(ZonedDateTime),
}

impl AlertTime {
    /// The day, either way.
    pub fn date(&self) -> civil::Date {
        match self {
            AlertTime::Date(d) => *d,
            AlertTime::At(t) => t.date(),
        }
    }

    /// The exact time, if Customer Alerts gave one.
    pub fn time(&self) -> Option<&ZonedDateTime> {
        match self {
            AlertTime::Date(_) => None,
            AlertTime::At(t) => Some(t),
        }
    }

    /// `"2025-12-15"` or `"2026-11-22T09:00:00"` (Chicago local time).
    fn parse(s: &str, tz: &TimeZone) -> Result<Self, DecodeError> {
        let s = s.trim();
        if s.contains('T') {
            let dt: civil::DateTime = s.parse()?;
            Ok(AlertTime::At(dt.to_zoned_datetime(tz.clone())?))
        } else {
            Ok(AlertTime::Date(s.parse()?))
        }
    }
}

impl PartialOrd for AlertTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AlertTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.date()
            .cmp(&other.date())
            .then_with(|| match (self, other) {
                (AlertTime::Date(_), AlertTime::Date(_)) => Ordering::Equal,
                (AlertTime::Date(_), AlertTime::At(_)) => Ordering::Less,
                (AlertTime::At(_), AlertTime::Date(_)) => Ordering::Greater,
                (AlertTime::At(a), AlertTime::At(b)) => a.cmp(b),
            })
    }
}

/// A train or bus route to ask for alerts about, for [`AlertsQuery::routes`].
///
/// Converts from either kind of route, so one list can hold both:
///
/// ```
/// use cta::alerts::RouteId;
/// use cta::trains::Route;
///
/// let routes = [RouteId::from(Route::Brown), RouteId::from("22")];
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RouteId {
    /// A train line.
    Train(trains::Route),
    /// A bus route.
    Bus(buses::Route),
}

impl RouteId {
    fn code(&self) -> &str {
        match self {
            RouteId::Train(r) => r.code(),
            RouteId::Bus(r) => r.as_str(),
        }
    }
}

impl From<trains::Route> for RouteId {
    fn from(r: trains::Route) -> Self {
        RouteId::Train(r)
    }
}

impl From<buses::Route> for RouteId {
    fn from(r: buses::Route) -> Self {
        RouteId::Bus(r)
    }
}

/// A bus route, e.g. `"22"`.
impl From<&str> for RouteId {
    fn from(r: &str) -> Self {
        RouteId::Bus(r.into())
    }
}

/// A bus route, e.g. `"22"`.
impl From<String> for RouteId {
    fn from(r: String) -> Self {
        RouteId::Bus(r.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AlertsTarget {
    All,
    Routes(Vec<RouteId>),
    Station(StationId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StartFilter {
    None,
    WithinDays(u32),
    Before(civil::Date),
}

/// Which alerts to fetch: every alert, alerts about some routes, or about one station,
/// with optional filters.
///
/// Without filters, this includes upcoming alerts as well as active ones.
///
/// ```
/// use cta::alerts::{AlertsQuery, RouteId};
/// use cta::trains::{Route, StationId};
///
/// let everything = AlertsQuery::all();
/// let some = AlertsQuery::routes([RouteId::from(Route::Brown), RouteId::from("22")])?
///     .active_only();
/// let clark_lake = AlertsQuery::station(StationId(40380)).without_planned();
/// let recent = AlertsQuery::all().started_within_days(7);
/// # Ok::<(), cta::alerts::QueryError>(())
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertsQuery {
    target: AlertsTarget,
    active_only: bool,
    accessibility: bool,
    planned: bool,
    start: StartFilter,
}

impl AlertsQuery {
    fn new(target: AlertsTarget) -> Self {
        Self {
            target,
            active_only: false,
            accessibility: true,
            planned: true,
            start: StartFilter::None,
        }
    }

    /// Every alert.
    pub fn all() -> Self {
        Self::new(AlertsTarget::All)
    }

    /// Alerts about these train and bus routes. Duplicates are ignored.
    ///
    /// # Errors
    ///
    /// [`QueryError::NoRoutes`] if `routes` is empty.
    pub fn routes<R: Into<RouteId>>(
        routes: impl IntoIterator<Item = R>,
    ) -> Result<Self, QueryError> {
        let routes = dedup(routes.into_iter().map(Into::into));
        if routes.is_empty() {
            return Err(QueryError::NoRoutes);
        }
        Ok(Self::new(AlertsTarget::Routes(routes)))
    }

    /// Alerts about one train station, and about every line serving it. Customer Alerts
    /// accepts only one station per request.
    ///
    /// To tell them apart, check whether an alert's [`services`](Alert::services)
    /// include [`Service::Station`] for this station.
    pub fn station(station: StationId) -> Self {
        Self::new(AlertsTarget::Station(station))
    }

    /// Only alerts that have started and haven't ended.
    pub fn active_only(mut self) -> Self {
        self.active_only = true;
        self
    }

    /// Leave out alerts about accessible paths in stations, such as elevator outages.
    pub fn without_accessibility(mut self) -> Self {
        self.accessibility = false;
        self
    }

    /// Leave out common planned alerts, such as planned work and reroutes.
    pub fn without_planned(mut self) -> Self {
        self.planned = false;
        self
    }

    /// Only alerts that started within the last `days` days. `0` means no limit, as in
    /// Customer Alerts.
    ///
    /// Replaces an earlier [`starting_before`](Self::starting_before).
    pub fn started_within_days(mut self, days: u32) -> Self {
        self.start = if days == 0 {
            StartFilter::None
        } else {
            StartFilter::WithinDays(days)
        };
        self
    }

    /// Only alerts that start before `date`.
    ///
    /// Replaces an earlier [`started_within_days`](Self::started_within_days).
    pub fn starting_before(mut self, date: civil::Date) -> Self {
        self.start = StartFilter::Before(date);
        self
    }
}

/// Build the request for the alerts in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn detailed_request(base: &Url, query: &AlertsQuery) -> Request {
    let mut url = base.join("alerts.aspx").expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        match &query.target {
            AlertsTarget::All => {}
            AlertsTarget::Routes(routes) => {
                let codes: Vec<_> = routes.iter().map(RouteId::code).collect();
                q.append_pair("routeid", &codes.join(","));
            }
            AlertsTarget::Station(station) => {
                q.append_pair("stationid", &station.to_string());
            }
        }
        if query.active_only {
            q.append_pair("activeonly", "true");
        }
        if !query.accessibility {
            q.append_pair("accessibility", "false");
        }
        if !query.planned {
            q.append_pair("planned", "false");
        }
        match query.start {
            StartFilter::None => {}
            StartFilter::WithinDays(days) => {
                q.append_pair("recentdays", &days.to_string());
            }
            StartFilter::Before(date) => {
                q.append_pair("bystartdate", &date.strftime("%Y%m%d").to_string());
            }
        }
        q.append_pair("outputType", "JSON");
    }
    Request { url }
}

/// Parse the body of a detailed alerts response.
///
/// No matching alerts is an empty list, not an error.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, or [`ResponseError::Decode`] if
/// the body couldn't be read.
pub fn parse_detailed(body: &[u8]) -> Result<Vec<Alert>, ResponseError> {
    let raw: RawAlerts = parse_envelope(body, "CTAAlerts")?;
    let tz = chicago()?;
    Ok(raw
        .alert
        .into_iter()
        .map(|a| to_alert(a, &tz))
        .collect::<Result<_, _>>()?)
}

fn to_alert(a: RawAlert, tz: &TimeZone) -> Result<Alert, DecodeError> {
    Ok(Alert {
        id: AlertId(a.alert_id),
        guid: a
            .guid
            .map(|g| g.trim().to_owned())
            .filter(|g| !g.is_empty()),
        headline: a.headline.trim().to_owned(),
        short_description: a.short_description.trim().to_owned(),
        full_description: a.full_description.unwrap_or_default(),
        impact: a.impact.trim().to_owned(),
        severity: Severity {
            score: a.severity_score.unwrap_or(0),
            category: SeverityCategory::from_cta(&a.severity_css),
            color: normalize_color(&a.severity_color),
        },
        start: AlertTime::parse(&a.event_start, tz)?,
        end: match a.event_end.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(end) => Some(AlertTime::parse(end, tz)?),
        },
        major: a.major_alert.unwrap_or(0) != 0,
        url: a.alert_url,
        services: a
            .impacted_service
            .map(|s| s.service)
            .unwrap_or_default()
            .into_iter()
            .map(to_affected_service)
            .collect(),
        ttim: a.ttim.unwrap_or(0) != 0,
    })
}

fn to_affected_service(s: RawService) -> AffectedService {
    let id = s.service_id.trim();
    let name = s.service_name.trim();
    let service = match s.service_type.trim() {
        "B" if !id.is_empty() => Service::BusRoute(buses::Route::from(id)),
        "R" => trains::Route::ALL
            .into_iter()
            .find(|r| r.code().eq_ignore_ascii_case(id))
            .map_or_else(|| Service::Other(id.to_owned()), Service::TrainRoute),
        "T" => id.parse().map_or_else(
            |_| Service::Other(id.to_owned()),
            |n| Service::Station(StationId(n)),
        ),
        "X" => Systemwide::from_id(id)
            .or_else(|| Systemwide::from_name(name))
            .map_or_else(|| Service::Other(id.to_owned()), Service::Systemwide),
        _ => Service::Other(id.to_owned()),
    };
    AffectedService {
        service,
        name: name.to_owned(),
        color: normalize_color(&s.service_back_color),
        text_color: normalize_color(&s.service_text_color),
        url: s.service_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Api, ApiErrorKind};
    use jiff::civil::date;

    const ALERTS: &[u8] = include_bytes!("../../tests/fixtures/alerts_alerts.json");
    const NONE: &[u8] = include_bytes!("../../tests/fixtures/alerts_no_results.json");

    fn query_string(query: &AlertsQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = detailed_request(&base, query).url;
        assert_eq!(url.path(), "/api/1.0/alerts.aspx");
        url.query().unwrap().to_owned()
    }

    fn alert(all: &[Alert], id: u32) -> &Alert {
        all.iter()
            .find(|a| a.id == AlertId(id))
            .unwrap_or_else(|| panic!("no alert {id}"))
    }

    fn chicago_time(y: i16, mo: i8, d: i8, h: i8, mi: i8) -> ZonedDateTime {
        date(y, mo, d)
            .at(h, mi, 0, 0)
            .to_zoned_datetime(chicago().unwrap())
            .unwrap()
    }

    #[test]
    fn requests() {
        assert_eq!(query_string(&AlertsQuery::all()), "outputType=JSON");
        let q = AlertsQuery::routes([
            RouteId::from(trains::Route::Brown),
            RouteId::from("22"),
            RouteId::from(trains::Route::Brown),
        ])
        .unwrap()
        .active_only()
        .without_accessibility()
        .without_planned()
        .started_within_days(7);
        assert_eq!(
            query_string(&q),
            "routeid=Brn%2C22&activeonly=true&accessibility=false&planned=false&recentdays=7&outputType=JSON"
        );
        let q = AlertsQuery::station(StationId(40380)).starting_before(date(2026, 12, 1));
        assert_eq!(
            query_string(&q),
            "stationid=40380&bystartdate=20261201&outputType=JSON"
        );
    }

    #[test]
    fn date_filters_replace_each_other() {
        let q = AlertsQuery::all()
            .starting_before(date(2026, 12, 1))
            .started_within_days(3);
        assert_eq!(query_string(&q), "recentdays=3&outputType=JSON");
        let q = AlertsQuery::all()
            .started_within_days(3)
            .starting_before(date(2026, 12, 1));
        assert_eq!(query_string(&q), "bystartdate=20261201&outputType=JSON");
        // 0 means no limit, as in Customer Alerts.
        let q = AlertsQuery::all()
            .started_within_days(3)
            .started_within_days(0);
        assert_eq!(query_string(&q), "outputType=JSON");
    }

    #[test]
    fn empty_route_list_is_error() {
        assert_eq!(
            AlertsQuery::routes(Vec::<RouteId>::new()),
            Err(QueryError::NoRoutes)
        );
    }

    #[test]
    fn parses_alert() {
        let all = parse_detailed(ALERTS).unwrap();
        assert_eq!(all.len(), 53);
        let a = alert(&all, 93306);
        assert_eq!(
            a.guid.as_deref(),
            Some("664b81c1-197b-450b-a00c-090483b90bb9")
        );
        assert_eq!(a.headline, "Temporary Bus Stop Change");
        assert!(
            a.short_description
                .starts_with("The SB #22 and #24 bus stop")
        );
        assert!(
            a.full_description
                .starts_with("<p><strong>How does this affect my trip?")
        );
        assert_eq!(a.impact, "Bus Stop Relocation");
        assert_eq!(
            a.severity,
            Severity {
                score: 9,
                category: SeverityCategory::Planned,
                color: "#0066cc".into()
            }
        );
        // A planned alert can score in the informational band.
        assert_eq!(a.severity.band(), SeverityBand::Informational);
        assert_eq!(a.start, AlertTime::At(chicago_time(2022, 7, 11, 9, 0)));
        assert_eq!(a.end, None);
        assert!(!a.major);
        assert!(!a.ttim);
        assert_eq!(
            a.url.as_deref(),
            Some(
                "http://www.transitchicago.com/travel_information/alert_detail.aspx?AlertId=93306"
            )
        );
        let services: Vec<_> = a
            .services
            .iter()
            .map(|s| (&s.service, s.name.as_str()))
            .collect();
        assert_eq!(
            services,
            [
                (&Service::BusRoute("22".into()), "Clark"),
                (&Service::BusRoute("24".into()), "Wentworth"),
            ]
        );
        assert_eq!(a.services[0].color, "#565a5c");
        assert_eq!(
            a.services[0].url.as_deref(),
            Some("http://www.transitchicago.com/bus/22/")
        );
    }

    #[test]
    fn parses_date_only_times_and_a_single_service() {
        let all = parse_detailed(ALERTS).unwrap();
        let a = alert(&all, 111296);
        assert_eq!(a.start, AlertTime::Date(date(2025, 11, 7)));
        assert_eq!(a.end, Some(AlertTime::Date(date(2026, 12, 31))));
        assert_eq!(a.start.time(), None);
        // Sent as one object rather than an array.
        assert_eq!(a.services.len(), 1);
        assert_eq!(a.services[0].service, Service::BusRoute("8".into()));
    }

    #[test]
    fn parses_train_routes_stations_and_categories() {
        let all = parse_detailed(ALERTS).unwrap();
        let closure = alert(&all, 112148);
        assert!(closure.ttim);
        assert_eq!(closure.severity.category, SeverityCategory::SpecialNote);
        let lines: Vec<_> = closure.services.iter().map(|s| s.service.clone()).collect();
        use trains::Route::*;
        assert_eq!(
            lines,
            [Purple, Pink, Green, Brown, Orange].map(Service::TrainRoute)
        );
        let minor = alert(&all, 118705);
        assert_eq!(minor.severity.score, 55);
        assert_eq!(minor.severity.category, SeverityCategory::Minor);
        assert_eq!(minor.severity.band(), SeverityBand::MinorDelays);
        assert_eq!(minor.severity.color, "#b45f04");

        let services = all.iter().flat_map(|a| &a.services);
        assert!(
            services
                .clone()
                .any(|s| matches!(s.service, Service::Station(StationId(40380))))
        );
        assert!(
            services
                .clone()
                .all(|s| !matches!(s.service, Service::Other(_))),
            "every affected service is classified"
        );
        // Short descriptions often end in "\r\n".
        assert!(
            all.iter()
                .all(|a| a.short_description.trim() == a.short_description)
        );
    }

    #[test]
    fn severity_bands() {
        let band = |score| {
            Severity {
                score,
                category: SeverityCategory::Normal,
                color: String::new(),
            }
            .band()
        };
        assert_eq!(band(0), SeverityBand::Informational);
        assert_eq!(band(19), SeverityBand::Informational);
        assert_eq!(band(20), SeverityBand::Planned);
        assert_eq!(band(39), SeverityBand::Planned);
        assert_eq!(band(40), SeverityBand::MinorDelays);
        assert_eq!(band(60), SeverityBand::SignificantDelays);
        assert_eq!(band(80), SeverityBand::MajorDisruption);
        assert_eq!(band(99), SeverityBand::MajorDisruption);
        assert_eq!(band(255), SeverityBand::MajorDisruption);
    }

    #[test]
    fn categories() {
        for (css, cat) in [
            ("normal", SeverityCategory::Normal),
            ("Planned", SeverityCategory::Planned),
            ("minor", SeverityCategory::Minor),
            ("MAJOR", SeverityCategory::Major),
            ("special-note", SeverityCategory::SpecialNote),
            ("urgent", SeverityCategory::Other("urgent".into())),
        ] {
            assert_eq!(SeverityCategory::from_cta(css), cat, "{css}");
        }
    }

    #[test]
    fn alert_times_sort_by_day_then_time() {
        let day = AlertTime::Date(date(2026, 11, 22));
        let morning = AlertTime::At(chicago_time(2026, 11, 22, 9, 0));
        let evening = AlertTime::At(chicago_time(2026, 11, 22, 21, 0));
        let earlier = AlertTime::At(chicago_time(2026, 11, 21, 23, 0));
        let later_day = AlertTime::Date(date(2026, 11, 23));
        let mut times = vec![
            later_day.clone(),
            evening.clone(),
            day.clone(),
            morning.clone(),
            earlier.clone(),
        ];
        times.sort();
        assert_eq!(times, [earlier, day, morning, evening, later_day]);
    }

    #[test]
    fn systemwide_services() {
        // The guide says systemwide groups may come without an ID, so names work too.
        let body = br#"{"CTAAlerts": {"ErrorCode": "0", "Alert": {
            "AlertId": "1", "Headline": " Weather ", "ShortDescription": "s", "EventStart": "2026-11-22",
            "SeverityScore": "50", "SeverityCSS": "minor", "SeverityColor": "B45F04", "MajorAlert": "1",
            "ImpactedService": {"Service": [
                {"ServiceType": "X", "ServiceName": "All Bus Routes", "ServiceId": ""},
                {"ServiceType": "X", "ServiceName": "All Routes", "ServiceId": "Systemwide"},
                {"ServiceType": "R", "ServiceName": "Purple Line Express", "ServiceId": "Pexp"},
                {"ServiceType": "Q", "ServiceName": "?", "ServiceId": "q"}
            ]}
        }}}"#;
        let a = parse_detailed(body).unwrap().remove(0);
        assert_eq!(a.headline, "Weather");
        assert!(a.major);
        assert_eq!(a.full_description, "");
        assert_eq!((a.guid.as_deref(), a.url.as_deref()), (None, None));
        let services: Vec<_> = a.services.iter().map(|s| s.service.clone()).collect();
        assert_eq!(
            services,
            [
                Service::Systemwide(Systemwide::AllBusRoutes),
                Service::Systemwide(Systemwide::AllRoutes),
                Service::Other("Pexp".into()),
                Service::Other("q".into()),
            ]
        );
    }

    #[test]
    fn no_alerts_is_empty() {
        assert!(parse_detailed(NONE).unwrap().is_empty());
    }

    #[test]
    fn bad_time_is_decode_error() {
        let body = br#"{"CTAAlerts": {"Alert": {"AlertId": "1", "EventStart": "soon"}}}"#;
        assert!(matches!(
            parse_detailed(body),
            Err(ResponseError::Decode(_))
        ));
    }

    #[test]
    fn errors_are_api_errors() {
        let body = br#"{"CTAAlerts": {"ErrorCode": "106", "ErrorMessage": "Invalid option: 'routeid' and 'stationid' parameters cannot be used together"}}"#;
        match parse_detailed(body) {
            Err(ResponseError::Api(e)) => {
                assert_eq!(
                    (e.api, e.code, e.kind()),
                    (Api::Alerts, Some(106), ApiErrorKind::Other)
                );
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }
}
