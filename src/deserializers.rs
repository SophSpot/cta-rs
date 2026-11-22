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

/// A JSON value that Bus Tracker sends as a number in some responses and a string in
/// others, e.g. `"lat": 41.88` in stops but `"lat": "41.88"` in vehicles.
#[derive(Deserialize)]
#[serde(untagged)]
enum NumOrStr<T> {
    Num(T),
    Str(String),
}

/// Reads a number sent either as a JSON number or as a string, e.g. `456` or `"456"`.
pub fn num_or_str<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr + Deserialize<'de>,
    T::Err: Display,
{
    match NumOrStr::<T>::deserialize(d)? {
        NumOrStr::Num(n) => Ok(n),
        NumOrStr::Str(s) => s.trim().parse().map_err(serde::de::Error::custom),
    }
}

/// Like [`num_or_str`], but a missing value, `null`, or `""` is `None`.
pub fn opt_num_or_str<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr + Deserialize<'de>,
    T::Err: Display,
{
    match Option::<NumOrStr<T>>::deserialize(d)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) if s.trim().is_empty() => Ok(None),
        Some(NumOrStr::Str(s)) => s.trim().parse().map(Some).map_err(serde::de::Error::custom),
    }
}

/// Reads an optional boolean sent as `true`/`false` or as `"true"`/`"false"` (any case).
/// A missing value, `null`, or `""` is `None`.
pub fn opt_bool_or_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrStr {
        Bool(bool),
        Str(String),
    }

    match Option::<BoolOrStr>::deserialize(d)? {
        None => Ok(None),
        Some(BoolOrStr::Bool(b)) => Ok(Some(b)),
        Some(BoolOrStr::Str(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(serde::de::Error::custom(format!("not a boolean: {s:?}"))),
        },
    }
}

/// Reads a Bus Tracker local time, `"YYYYMMDD HH:MM:SS"` or `"YYYYMMDD HH:MM"`.
///
/// Bus Tracker sends seconds when a request has `tmres=s`, and minutes otherwise. Some
/// fields, such as detour start and end times, are always to the minute.
pub fn bus_time<'de, D: serde::Deserializer<'de>>(d: D) -> Result<jiff::civil::DateTime, D::Error> {
    let s = String::deserialize(d)?;
    parse_bus_time(&s).map_err(serde::de::Error::custom)
}

/// Like [`bus_time`], but a missing value, `null`, or `""` is `None`.
pub fn opt_bus_time<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<jiff::civil::DateTime>, D::Error> {
    match Option::<String>::deserialize(d)? {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => parse_bus_time(&s)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
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

    #[derive(Debug, Deserialize)]
    struct Nums {
        #[serde(deserialize_with = "num_or_str")]
        lat: f64,
        #[serde(deserialize_with = "num_or_str")]
        id: u32,
        #[serde(default, deserialize_with = "opt_num_or_str")]
        seq: Option<u32>,
        #[serde(default, deserialize_with = "opt_bool_or_str")]
        ada: Option<bool>,
    }

    fn nums(json: &str) -> Result<Nums, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn numbers_or_strings() {
        let n = nums(r#"{"lat": 41.88, "id": 456}"#).unwrap();
        assert_eq!((n.lat, n.id, n.seq, n.ada), (41.88, 456, None, None));
        let n = nums(r#"{"lat": "41.88", "id": "456", "seq": "7", "ada": "TRUE"}"#).unwrap();
        assert_eq!(
            (n.lat, n.id, n.seq, n.ada),
            (41.88, 456, Some(7), Some(true))
        );
        let n = nums(r#"{"lat": 1, "id": 1, "seq": 7, "ada": false}"#).unwrap();
        assert_eq!((n.seq, n.ada), (Some(7), Some(false)));
    }

    #[test]
    fn empty_and_null_optionals_are_none() {
        let n = nums(r#"{"lat": 1, "id": 1, "seq": "", "ada": ""}"#).unwrap();
        assert_eq!((n.seq, n.ada), (None, None));
        let n = nums(r#"{"lat": 1, "id": 1, "seq": null, "ada": null}"#).unwrap();
        assert_eq!((n.seq, n.ada), (None, None));
    }

    #[test]
    fn bad_numbers_and_booleans_are_errors() {
        assert!(nums(r#"{"lat": "north", "id": 1}"#).is_err());
        assert!(nums(r#"{"lat": 1, "id": "sad"}"#).is_err());
        assert!(nums(r#"{"lat": 1, "id": -1}"#).is_err());
        assert!(nums(r#"{"lat": 1, "id": 1, "ada": "yes"}"#).is_err());
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
