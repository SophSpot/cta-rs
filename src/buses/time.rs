use super::parse_envelope;
use crate::deserializers::bus_time;
use crate::{DecodeError, Request, ResponseError, chicago};
use jiff::{ZonedDateTime, civil};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawTime {
    #[serde(deserialize_with = "bus_time")]
    tm: civil::DateTime,
}

/// Build the request for Bus Tracker's current time, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn time_request(base: &Url, key: &str) -> Request {
    let mut url = base.join("gettime").expect("base URL must be http(s)");
    url.query_pairs_mut()
        .append_pair("key", key)
        .append_pair("format", "json");
    Request { url }
}

/// Parse the body of a time response.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_time(body: &[u8]) -> Result<ZonedDateTime, ResponseError> {
    let raw: RawTime = parse_envelope(body)?;
    let tz = chicago()?;
    Ok(raw.tm.to_zoned_datetime(tz).map_err(DecodeError::from)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Api, ApiErrorKind};

    const TIME: &[u8] = include_bytes!("../../tests/fixtures/bus_time.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    #[test]
    fn request_url() {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let url = time_request(&base, "k").url;
        assert_eq!(
            url.as_str(),
            "https://www.ctabustracker.com/bustime/api/v3/gettime?key=k&format=json"
        );
    }

    #[test]
    fn request_debug_hides_key() {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        let out = format!("{:?}", time_request(&base, "secret"));
        assert!(!out.contains("secret"), "{out}");
        assert!(out.contains("REDACTED"), "{out}");
    }

    #[test]
    fn parses_chicago_time() {
        let tm = parse_time(TIME).unwrap();
        assert_eq!(tm.time_zone().iana_name(), Some("America/Chicago"));
        assert_eq!(tm.datetime(), civil::date(2026, 11, 22).at(10, 16, 42, 0));
        // November is Central Standard Time.
        assert_eq!(tm.offset(), jiff::tz::offset(-6));
    }

    #[test]
    fn parses_daylight_time() {
        let body = br#"{"bustime-response": {"tm": "20160708 14:11:54"}}"#;
        assert_eq!(parse_time(body).unwrap().offset(), jiff::tz::offset(-5));
    }

    #[test]
    fn bad_key_is_api_error() {
        match parse_time(BAD_KEY) {
            Err(ResponseError::Api(e)) => {
                assert_eq!(e.api, Api::Bus);
                assert_eq!(e.kind(), ApiErrorKind::InvalidKey);
            }
            other => panic!("expected ApiError, got {other:?}"),
        }
    }

    #[test]
    fn unix_time_is_decode_error() {
        // What gettime returns with unixTime=true, which this crate never sends.
        let body = br#"{"bustime-response": {"tm": "1531860021189"}}"#;
        assert!(matches!(parse_time(body), Err(ResponseError::Decode(_))));
    }
}
