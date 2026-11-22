use super::{Direction, Locale, Route, append_locale, parse_envelope};
use crate::deserializers::one_or_many;
use crate::{Request, ResponseError};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawDirections {
    #[serde(default, deserialize_with = "one_or_many")]
    directions: Vec<RawDirection>,
}

#[derive(Deserialize)]
struct RawDirection {
    id: String,
    name: String,
}

/// A direction a route runs in, as listed by
/// [`Buses::directions`](crate::client::Buses::directions).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DirectionInfo {
    /// The direction. Pass it to other calls, such as stops for a route and direction.
    pub direction: Direction,
    /// The direction's name for showing to riders, e.g. `"Eastbound"`.
    ///
    /// Bus Tracker can translate this with [`DirectionsQuery::locale`], but CTA returns
    /// English in every locale today, so it matches [`direction`](DirectionInfo::direction).
    pub name: String,
}

/// Which route to list directions for.
///
/// ```
/// use cta::buses::DirectionsQuery;
///
/// let madison = DirectionsQuery::route("20");
/// let spanish = DirectionsQuery::route("20").locale("es");
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectionsQuery {
    route: Route,
    locale: Option<Locale>,
}

impl DirectionsQuery {
    /// The directions `route` runs in.
    pub fn route(route: impl Into<Route>) -> Self {
        Self {
            route: route.into(),
            locale: None,
        }
    }

    /// The language for direction names. See [`Locale`] for what CTA translates today.
    pub fn locale(mut self, locale: impl Into<Locale>) -> Self {
        self.locale = Some(locale.into());
        self
    }
}

/// Build the request for the directions in `query`, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn directions_request(base: &Url, key: &str, query: &DirectionsQuery) -> Request {
    let mut url = base
        .join("getdirections")
        .expect("base URL must be http(s)");
    url.query_pairs_mut()
        .append_pair("key", key)
        .append_pair("rt", query.route.as_str());
    append_locale(&mut url, query.locale.as_ref());
    url.query_pairs_mut().append_pair("format", "json");
    Request { url }
}

/// Parse the body of a directions response.
///
/// Most routes run in two directions, but some run in one: route 19 (United Center
/// Express) is Eastbound only. An unknown route has no directions.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_directions(body: &[u8]) -> Result<Vec<DirectionInfo>, ResponseError> {
    let raw: RawDirections = parse_envelope(body)?;
    Ok(raw
        .directions
        .into_iter()
        .map(|d| DirectionInfo {
            direction: Direction::from(d.id),
            name: d.name,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const MADISON: &[u8] = include_bytes!("../../tests/fixtures/bus_directions_20.json");
    const UNITED_CENTER: &[u8] = include_bytes!("../../tests/fixtures/bus_directions_19.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn base() -> Url {
        Url::parse(super::super::DEFAULT_BASE_URL).unwrap()
    }

    #[test]
    fn request_url() {
        assert_eq!(
            directions_request(&base(), "k", &DirectionsQuery::route("X9"))
                .url
                .as_str(),
            "https://www.ctabustracker.com/bustime/api/v3/getdirections?key=k&rt=X9&format=json"
        );
    }

    #[test]
    fn request_url_with_locale() {
        let q = DirectionsQuery::route("20").locale("es");
        assert_eq!(
            directions_request(&base(), "k", &q).url.as_str(),
            "https://www.ctabustracker.com/bustime/api/v3/getdirections?key=k&rt=20&locale=es&format=json"
        );
    }

    #[test]
    fn parses_two_directions() {
        let dirs = parse_directions(MADISON).unwrap();
        assert_eq!(
            dirs,
            [
                DirectionInfo {
                    direction: Direction::Eastbound,
                    name: "Eastbound".into()
                },
                DirectionInfo {
                    direction: Direction::Westbound,
                    name: "Westbound".into()
                },
            ]
        );
    }

    #[test]
    fn parses_one_direction() {
        let dirs = parse_directions(UNITED_CENTER).unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].direction, Direction::Eastbound);
    }

    #[test]
    fn keeps_unknown_direction_and_localized_name() {
        let body = br#"{"bustime-response": {"directions": [
            {"id": "Clockwise", "name": "Sentido horario"}
        ]}}"#;
        let dirs = parse_directions(body).unwrap();
        assert_eq!(dirs[0].direction, Direction::Other("Clockwise".into()));
        assert_eq!(dirs[0].name, "Sentido horario");
    }

    #[test]
    fn unknown_route_is_empty() {
        let body = br#"{"bustime-response": {"error": [
            {"rt": "999", "msg": "No data found for parameter"}
        ]}}"#;
        assert!(parse_directions(body).unwrap().is_empty());
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_directions(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
