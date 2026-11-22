use serde::Deserialize;
use std::fmt::Display;
use std::str::FromStr;

pub fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(<String>::deserialize(d)? == "1")
}

/// Reads a JSON string and parses it with `FromStr`, e.g. `"40380"` -> `40380u32`.
pub fn from_str<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    String::deserialize(d)?
        .parse()
        .map_err(serde::de::Error::custom)
}

pub fn opt_from_str<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    match Option::<String>::deserialize(d)? {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

pub fn one_or_many<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        Many(Vec<T>),
        One(T),
    }

    Ok(match Option::<OneOrMany<T>>::deserialize(d)? {
        None => Vec::new(),
        Some(OneOrMany::Many(v)) => v,
        Some(OneOrMany::One(t)) => vec![t],
    })
}

/// Reads a Bus Tracker local time, `"YYYYMMDD HH:MM:SS"` or `"YYYYMMDD HH:MM"`.
///
/// Bus Tracker sends seconds when a request has `tmres=s`, and minutes otherwise. Some
/// fields, such as detour start and end times, are always to the minute.
pub fn bus_time<'de, D: serde::Deserializer<'de>>(d: D) -> Result<jiff::civil::DateTime, D::Error> {
    let s = String::deserialize(d)?;
    parse_bus_time(&s).map_err(serde::de::Error::custom)
}

pub fn parse_bus_time(s: &str) -> Result<jiff::civil::DateTime, jiff::Error> {
    jiff::civil::DateTime::strptime("%Y%m%d %H:%M:%S", s)
        .or_else(|_| jiff::civil::DateTime::strptime("%Y%m%d %H:%M", s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn bus_time_with_seconds() {
        assert_eq!(
            parse_bus_time("20261122 10:16:42").unwrap(),
            date(2026, 11, 22).at(10, 16, 42, 0)
        );
    }

    #[test]
    fn bus_time_with_minutes() {
        assert_eq!(
            parse_bus_time("20261122 10:16").unwrap(),
            date(2026, 11, 22).at(10, 16, 0, 0)
        );
    }

    #[test]
    fn bus_time_rejects_other_formats() {
        for s in [
            "",
            "2026-11-22T10:16:42",
            "20261122",
            "20261122 25:00",
            "1531860021189",
        ] {
            assert!(parse_bus_time(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn bus_time_deserializes_from_json() {
        #[derive(Deserialize)]
        struct T {
            #[serde(deserialize_with = "bus_time")]
            tm: jiff::civil::DateTime,
        }
        let t: T = serde_json::from_str(r#"{"tm":"20160308 14:11:54"}"#).unwrap();
        assert_eq!(t.tm, date(2016, 3, 8).at(14, 11, 54, 0));
    }
}
