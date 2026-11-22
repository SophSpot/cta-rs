use super::{Locale, parse_envelope};
use crate::deserializers::one_or_many;
use crate::{Request, ResponseError};
use serde::Deserialize;
use url::Url;

#[derive(Deserialize)]
struct RawLocales {
    #[serde(default, deserialize_with = "one_or_many")]
    locale: Vec<RawLocale>,
}

#[derive(Deserialize)]
struct RawLocale {
    localestring: String,
    displayname: String,
}

/// A language Bus Tracker accepts, as listed by
/// [`Buses::locales`](crate::client::Buses::locales).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct LocaleInfo {
    /// The language code, e.g. `"es"`. Pass it to a query's `locale` method.
    pub locale: Locale,
    /// The language's name, e.g. `"Spanish"`, in the language chosen by the
    /// [`LocalesQuery`].
    pub name: String,
}

/// Which language the names in a locale list are in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LocaleNames {
    /// Bus Tracker's default language, English.
    Default,
    /// One language for every name (`locale=xx`).
    In(Locale),
    /// Each language named in itself (`inLocaleLanguage=true`).
    Native,
}

/// How to list the languages Bus Tracker accepts.
///
/// The list is always the same; the query only picks the language of the names.
///
/// ```
/// use cta::buses::LocalesQuery;
///
/// let english = LocalesQuery::all();                 // "English", "Spanish"
/// let spanish = LocalesQuery::all().names_in("es");  // "inglés", "español"
/// let native = LocalesQuery::all().native_names();   // "English", "español"
/// ```
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalesQuery {
    names: LocaleNames,
}

impl LocalesQuery {
    /// Every language, with names in English.
    pub fn all() -> Self {
        Self {
            names: LocaleNames::Default,
        }
    }

    /// Give every name in `locale`, e.g. `"español"` and `"inglés"` for `"es"`.
    ///
    /// Replaces an earlier [`native_names`](Self::native_names).
    pub fn names_in(mut self, locale: impl Into<Locale>) -> Self {
        self.names = LocaleNames::In(locale.into());
        self
    }

    /// Give each language's name in that language, e.g. `"English"` and `"español"`.
    /// Useful for a language picker.
    ///
    /// Replaces an earlier [`names_in`](Self::names_in).
    pub fn native_names(mut self) -> Self {
        self.names = LocaleNames::Native;
        self
    }
}

/// Build the request for the languages Bus Tracker accepts, without sending it.
///
/// `base` is usually [`DEFAULT_BASE_URL`](super::DEFAULT_BASE_URL).
///
/// # Panics
///
/// If `base` can't be a base URL, such as a `mailto:` URL.
pub fn locales_request(base: &Url, key: &str, query: &LocalesQuery) -> Request {
    let mut url = base
        .join("getlocalelist")
        .expect("base URL must be http(s)");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("key", key);
        match &query.names {
            LocaleNames::Default => {}
            LocaleNames::In(locale) => {
                q.append_pair("locale", locale.as_str());
            }
            LocaleNames::Native => {
                q.append_pair("inLocaleLanguage", "true");
            }
        }
        q.append_pair("format", "json");
    }
    Request { url }
}

/// Parse the body of a locale list response.
///
/// # Errors
///
/// [`ResponseError::Api`] if the CTA returned an error, such as an invalid key, or
/// [`ResponseError::Decode`] if the body couldn't be read.
pub fn parse_locales(body: &[u8]) -> Result<Vec<LocaleInfo>, ResponseError> {
    let raw: RawLocales = parse_envelope(body)?;
    Ok(raw
        .locale
        .into_iter()
        .map(|l| LocaleInfo {
            locale: Locale(l.localestring),
            name: l.displayname,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;

    const ENGLISH: &[u8] = include_bytes!("../../tests/fixtures/bus_locales.json");
    const SPANISH: &[u8] = include_bytes!("../../tests/fixtures/bus_locales_es.json");
    const NATIVE: &[u8] = include_bytes!("../../tests/fixtures/bus_locales_native.json");
    const BAD_KEY: &[u8] = include_bytes!("../../tests/fixtures/bus_bad_key.json");

    fn url(query: &LocalesQuery) -> String {
        let base = Url::parse(super::super::DEFAULT_BASE_URL).unwrap();
        locales_request(&base, "k", query).url.to_string()
    }

    fn names(body: &[u8]) -> Vec<(String, String)> {
        parse_locales(body)
            .unwrap()
            .into_iter()
            .map(|l| (l.locale.0, l.name))
            .collect()
    }

    fn pairs(items: [(&str, &str); 2]) -> Vec<(String, String)> {
        items
            .into_iter()
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect()
    }

    #[test]
    fn request_urls() {
        let base = "https://www.ctabustracker.com/bustime/api/v3/getlocalelist?key=k";
        assert_eq!(url(&LocalesQuery::all()), format!("{base}&format=json"));
        assert_eq!(
            url(&LocalesQuery::all().names_in("es")),
            format!("{base}&locale=es&format=json")
        );
        assert_eq!(
            url(&LocalesQuery::all().native_names()),
            format!("{base}&inLocaleLanguage=true&format=json")
        );
    }

    #[test]
    fn last_name_choice_wins() {
        assert_eq!(
            LocalesQuery::all().native_names().names_in("es"),
            LocalesQuery::all().names_in("es")
        );
        assert_eq!(
            LocalesQuery::all().names_in("es").native_names(),
            LocalesQuery::all().native_names()
        );
    }

    #[test]
    fn parses_live_responses() {
        assert_eq!(
            names(ENGLISH),
            pairs([("en", "English"), ("es", "Spanish")])
        );
        assert_eq!(names(SPANISH), pairs([("en", "inglés"), ("es", "español")]));
        assert_eq!(names(NATIVE), pairs([("en", "English"), ("es", "español")]));
    }

    #[test]
    fn bad_key_is_api_error() {
        assert!(matches!(
            parse_locales(BAD_KEY),
            Err(ResponseError::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }
}
