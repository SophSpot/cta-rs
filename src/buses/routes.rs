use super::{Locale, Route, append_locale, parse_envelope};
use crate::deserializers::one_or_many;
use crate::{Request, ResponseError};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawRoutes {
    #[serde(default, deserialize_with = "one_or_many")]
    routes: Vec<RawRoute>,
}

#[derive(Deserialize)]
struct RawRoute {
    rt: String,
    rtnm: String,
    rtclr: String,
    rtdd: String,
}

/// A bus route, as listed by [`Buses::routes`](crate::client::Buses::routes).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct RouteInfo {
    /// The route's code, e.g. `"20"`. Pass it to other calls.
    pub route: Route,
    /// The route's name, e.g. `"Madison"`.
    ///
    /// Bus Tracker can translate this with [`RoutesQuery::locale`], but CTA returns
    /// English in every locale today.
    pub name: String,
    /// The route's color on CTA maps, as `"#rrggbb"`, e.g. `"#336633"`.
    pub color: String,
    /// The route's label for showing to riders.
    ///
    /// Bus Tracker provides it separately from the code and can translate it with
    /// [`RoutesQuery::locale`], but it is the same as [`route`](RouteInfo::route) for
    /// every CTA route, in every locale, today.
    pub display: String,
}

/// Which routes to list. Bus Tracker always returns every route.
///
/// ```
/// use cta::buses::RoutesQuery;
///
/// let english = RoutesQuery::all();
/// let spanish = RoutesQuery::all().locale("es");
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutesQuery {
    locale: Option<Locale>,
}

impl RoutesQuery {
    /// Every route.
    pub fn all() -> Self {
        Self { locale: None }
    }

    /// The language for route names. See [`Locale`] for what CTA translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for the routes in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn routes_request(base: &Url, key: &str, query: &RoutesQuery) -> Request {
    let mut url = base.join("getroutes").expect("base URL must be http(s)");
    url.query_pairs_mut().append_pair("key", key);
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a routes response.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_routes(body: &[u8]) -> Result<Vec<RouteInfo>, ResponseError> {
    let raw: RawRoutes = parse_envelope(body)?;
    Ok(raw
        .routes
        .into_iter()
        .map(|r| RouteInfo {
            route: Route(r.rt),
            name: r.rtnm,
            color: r.rtclr,
            display: r.rtdd,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const ROUTES: &[u8] = include_bytes!("../../tests/fixtures/bus_routes.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn base() -> Url {
        Url::parse(super::super::DEFAULT_BASE_URL).unwrap()
    }

    #[test]
    fn request_url() {
        assert_eq!(
            routes_request(&base(), "k", &RoutesQuery::all())
                .url
                .as_str(),
            "https://www.ctabustracker.com/bustime/api/v3/getroutes?key=k&format=json"
        );
    }

    #[test]
    fn request_url_with_locale() {
        assert_eq!(
            routes_request(&base(), "k", &RoutesQuery::all().locale("es"))
                .url
                .as_str(),
            "https://www.ctabustracker.com/bustime/api/v3/getroutes?key=k&locale=es&format=json"
        );
    }

    #[test]
    fn parses_live_response() {
        let routes = parse_routes(ROUTES).unwrap();
        assert_eq!(routes.len(), 125);
        assert_eq!(
            routes[0],
            RouteInfo {
                route: Route::from("1"),
                name: "Bronzeville/Union Station".into(),
                color: "#336633".into(),
                display: "1".into(),
            }
        );
        // Codes that aren't plain numbers are kept as given.
        for code in ["X4", "N5", "8A"] {
            assert!(
                routes.iter().any(|r| r.route.as_str() == code),
                "missing {code}"
            );
        }
    }

    #[test]
    fn empty_response_is_empty() {
        let body = br#"{"bustime-response": {}}"#;
        assert!(parse_routes(body).unwrap().is_empty());
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_routes(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[test]
    fn missing_field_is_decode_error() {
        let body = br#"{"bustime-response": {"routes": [{"rt": "20", "rtnm": "Madison"}]}}"#;
        assert!(matches!(parse_routes(body), Err(ResponseError::Decode(_))));
    }
}
