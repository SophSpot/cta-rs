//! Async and blocking clients, and the traits for plugging in an HTTP client.

use crate::Error;
use crate::trains::{
    Arrival, ArrivalsQuery, DEFAULT_BASE_URL, FollowedTrain, LocationsQuery, TrainLocation,
    arrivals_request, follow_request, locations_request, parse_arrivals, parse_follow,
    parse_locations,
};
use std::fmt;
use url::Url;

/// An async HTTP client that [`AsyncClient`] can send requests with.
///
/// Implemented for `reqwest::Client` with the `reqwest` feature. Implement it yourself
/// to use another client, add logging or retries, or return canned responses in tests.
/// To implement it for a type from another crate, wrap that type in your own struct.
///
/// ```
/// use cta::{AsyncHttpClient, Url};
///
/// struct Canned(&'static str);
///
/// impl AsyncHttpClient for Canned {
///     type Error = std::io::Error;
///
///     async fn get(&self, _url: &Url) -> Result<Vec<u8>, Self::Error> {
///         Ok(self.0.as_bytes().to_vec())
///     }
/// }
///
/// let client = cta::AsyncClient::with_http("key", Canned(r#"{"ctatt":{"errCd":"0"}}"#));
/// ```
pub trait AsyncHttpClient {
    /// The error returned when a request fails.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Send a GET request to `url` and return the response body.
    ///
    /// A non-success HTTP status should be returned as an error. The URL contains the
    /// API key, so avoid logging it in full.
    fn get(&self, url: &Url) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send;
}

/// An async client for the CTA Train Tracker API.
///
/// Create one with [`new`](AsyncClient::new) (requires the `reqwest` feature) or
/// [`with_http`](AsyncClient::with_http). Cloning is cheap when the HTTP client's is,
/// as with `reqwest::Client`. The `Debug` output hides the API key.
#[derive(Clone)]
pub struct AsyncClient<H> {
    base_url: Url,
    key: String,
    http: H,
}

impl<H> fmt::Debug for AsyncClient<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncClient")
            .field("key", &"REDACTED")
            .field("base_url", &self.base_url.as_str())
            .finish_non_exhaustive()
    }
}

impl<H: AsyncHttpClient> AsyncClient<H> {
    /// Predicted arrivals at the stations or stops in `query`.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::trains::{ArrivalsQuery, Route, StationId};
    ///
    /// let query = ArrivalsQuery::stations([StationId(40380), StationId(40360)])?
    ///     .routes([Route::Blue, Route::Brown])
    ///     .max(5)?;
    /// let arrivals = client.arrivals(&query).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn arrivals(&self, query: &ArrivalsQuery) -> Result<Vec<Arrival>, Error<H::Error>> {
        let req = arrivals_request(&self.base_url, &self.key, query);
        let body = self.http.get(&req.url).await.map_err(Error::Http)?;
        Ok(parse_arrivals(&body)?)
    }

    /// Every train currently in service on the lines in `query`.
    pub async fn locations(
        &self,
        query: &LocationsQuery,
    ) -> Result<Vec<TrainLocation>, Error<H::Error>> {
        let req = locations_request(&self.base_url, &self.key, query);
        let body = self.http.get(&req.url).await.map_err(Error::Http)?;
        Ok(parse_locations(&body)?)
    }

    /// One train's position and its predicted arrival at each upcoming station.
    ///
    /// `run` is a run number, such as [`TrainLocation::run`] or
    /// [`Arrival::run`](crate::trains::Arrival::run). Runs end when a train reaches its
    /// terminal, so a recent run number can return
    /// [`ApiErrorKind::RunNotFound`](crate::ApiErrorKind::RunNotFound).
    pub async fn follow(&self, run: &str) -> Result<FollowedTrain, Error<H::Error>> {
        let req = follow_request(&self.base_url, &self.key, run);
        let body = self.http.get(&req.url).await.map_err(Error::Http)?;
        Ok(parse_follow(&body)?)
    }

    /// Create a client that sends requests with `http`.
    ///
    /// Use this to configure the HTTP client yourself (timeouts, proxies, user agent) or
    /// to share one with the rest of your application.
    pub fn with_http(key: impl Into<String>, http: H) -> Self {
        Self {
            key: key.into(),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("default base URL is valid"),
            http,
        }
    }

    /// Send requests to `url` instead of the CTA, for example a local mock server.
    ///
    /// A missing trailing `/` is added, so `http://localhost:8080/api/1.0` and
    /// `http://localhost:8080/api/1.0/` behave the same.
    pub fn base_url(mut self, mut url: Url) -> Self {
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        self.base_url = url;
        self
    }
}

#[cfg(feature = "reqwest")]
impl AsyncClient<reqwest::Client> {
    /// Create a client backed by a default `reqwest::Client`.
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_http(key, reqwest::Client::new())
    }
}

#[cfg(feature = "reqwest")]
impl AsyncHttpClient for reqwest::Client {
    type Error = reqwest::Error;
    fn get(&self, url: &Url) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send {
        let request = reqwest::Client::get(self, url.clone());

        async move {
            let result: Result<Vec<u8>, reqwest::Error> = async {
                let response = request.send().await?.error_for_status()?;
                Ok(response.bytes().await?.to_vec())
            }
            .await;
            result.map_err(reqwest::Error::without_url)
        }
    }
}

/// A blocking HTTP client that [`BlockingClient`] can send requests with.
///
/// Implemented for `ureq::Agent` with the `ureq` feature. Implement it yourself to use
/// another client. To implement it for a type from another crate, wrap that type in your
/// own struct.
pub trait BlockingHttpClient {
    /// The error returned when a request fails.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Send a GET request to `url` and return the response body.
    ///
    /// A non-success HTTP status should be returned as an error. The URL contains the
    /// API key, so avoid logging it in full.
    fn get(&self, url: &Url) -> Result<Vec<u8>, Self::Error>;
}

/// A blocking client for the CTA Train Tracker API.
///
/// The same API as [`AsyncClient`], without `async`. Create one with
/// [`new`](BlockingClient::new) (requires the `ureq` feature) or
/// [`with_http`](BlockingClient::with_http). The `Debug` output hides the API key.
#[derive(Clone)]
pub struct BlockingClient<H> {
    base_url: Url,
    key: String,
    http: H,
}

impl<H> fmt::Debug for BlockingClient<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockingClient")
            .field("key", &"REDACTED")
            .field("base_url", &self.base_url.as_str())
            .finish_non_exhaustive()
    }
}

impl<H: BlockingHttpClient> BlockingClient<H> {
    /// Predicted arrivals at the stations or stops in `query`.
    pub fn arrivals(&self, query: &ArrivalsQuery) -> Result<Vec<Arrival>, Error<H::Error>> {
        let req = arrivals_request(&self.base_url, &self.key, query);
        let body = self.http.get(&req.url).map_err(Error::Http)?;
        Ok(parse_arrivals(&body)?)
    }

    /// Every train currently in service on the lines in `query`.
    pub fn locations(&self, query: &LocationsQuery) -> Result<Vec<TrainLocation>, Error<H::Error>> {
        let req = locations_request(&self.base_url, &self.key, query);
        let body = self.http.get(&req.url).map_err(Error::Http)?;
        Ok(parse_locations(&body)?)
    }

    /// One train's position and its predicted arrival at each upcoming station.
    ///
    /// See [`AsyncClient::follow`] for what `run` is and when it can fail.
    pub fn follow(&self, run: &str) -> Result<FollowedTrain, Error<H::Error>> {
        let req = follow_request(&self.base_url, &self.key, run);
        let body = self.http.get(&req.url).map_err(Error::Http)?;
        Ok(parse_follow(&body)?)
    }

    /// Create a client that sends requests with `http`.
    pub fn with_http(key: impl Into<String>, http: H) -> Self {
        Self {
            key: key.into(),
            base_url: Url::parse(DEFAULT_BASE_URL).expect("default base URL is valid"),
            http,
        }
    }

    /// Send requests to `url` instead of the CTA. See [`AsyncClient::base_url`].
    pub fn base_url(mut self, mut url: Url) -> Self {
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        self.base_url = url;
        self
    }
}

#[cfg(feature = "ureq")]
impl BlockingHttpClient for ureq::Agent {
    type Error = ureq::Error;
    fn get(&self, url: &Url) -> Result<Vec<u8>, Self::Error> {
        let mut resp = self.get(url.as_str()).call()?;
        resp.body_mut().read_to_vec()
    }
}

#[cfg(feature = "ureq")]
impl BlockingClient<ureq::Agent> {
    /// Create a client backed by a default `ureq::Agent`.
    pub fn new(key: impl Into<String>) -> Self {
        Self::with_http(key, ureq::Agent::new_with_defaults())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiErrorKind;
    use crate::trains::{Route, StationId};
    use std::sync::{Arc, Mutex};

    const ARRIVALS: &[u8] = include_bytes!("../tests/fixtures/arrivals_clark_lake.json");
    const LOCATIONS: &[u8] = include_bytes!("../tests/fixtures/locations_yellow.json");
    const FOLLOW: &[u8] = include_bytes!("../tests/fixtures/follow_125.json");
    const BAD_KEY: &[u8] = br#"{"ctatt":{"errCd":"101","errNm":"Invalid API key"}}"#;

    #[derive(Debug, thiserror::Error)]
    #[error("connection refused")]
    struct Refused;

    /// Records each URL it's asked for and answers with a canned body, or fails.
    #[derive(Clone)]
    struct FakeHttp {
        body: Option<&'static [u8]>,
        seen: Arc<Mutex<Vec<Url>>>,
    }

    impl FakeHttp {
        fn ok(body: &'static [u8]) -> Self {
            Self {
                body: Some(body),
                seen: Arc::default(),
            }
        }
        fn failing() -> Self {
            Self {
                body: None,
                seen: Arc::default(),
            }
        }
        fn respond(&self, url: &Url) -> Result<Vec<u8>, Refused> {
            self.seen.lock().unwrap().push(url.clone());
            self.body.map(<[u8]>::to_vec).ok_or(Refused)
        }
        fn last_url(&self) -> Url {
            self.seen
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("a request was made")
        }
    }

    impl AsyncHttpClient for FakeHttp {
        type Error = Refused;
        async fn get(&self, url: &Url) -> Result<Vec<u8>, Self::Error> {
            self.respond(url)
        }
    }

    impl BlockingHttpClient for FakeHttp {
        type Error = Refused;
        fn get(&self, url: &Url) -> Result<Vec<u8>, Self::Error> {
            self.respond(url)
        }
    }

    // ---- AsyncClient ----

    #[tokio::test]
    async fn async_arrivals() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = AsyncClient::with_http("k", http.clone());
        let arrivals = client
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .await
            .unwrap();
        assert_eq!(arrivals.len(), 3);
        assert_eq!(http.last_url().path(), "/api/1.0/ttarrivals.aspx");
    }

    #[tokio::test]
    async fn async_locations_and_follow() {
        let client = AsyncClient::with_http("k", FakeHttp::ok(LOCATIONS));
        let trains = client
            .locations(&LocationsQuery::route(Route::Yellow))
            .await
            .unwrap();
        assert_eq!(trains.len(), 1);

        let client = AsyncClient::with_http("k", FakeHttp::ok(FOLLOW));
        assert_eq!(client.follow("125").await.unwrap().stops.len(), 3);
    }

    #[tokio::test]
    async fn async_errors_map_to_variants() {
        let q = ArrivalsQuery::at(StationId(40380));

        let client = AsyncClient::with_http("k", FakeHttp::failing());
        assert!(matches!(
            client.arrivals(&q).await,
            Err(Error::Http(Refused))
        ));

        let client = AsyncClient::with_http("k", FakeHttp::ok(BAD_KEY));
        assert!(matches!(
            client.arrivals(&q).await,
            Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));

        let client = AsyncClient::with_http("k", FakeHttp::ok(b"<html>oops</html>"));
        assert!(matches!(client.arrivals(&q).await, Err(Error::Decode(_))));
    }

    #[tokio::test]
    async fn async_base_url_override_adds_trailing_slash() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = AsyncClient::with_http("k", http.clone())
            .base_url(Url::parse("http://localhost:8080/api/1.0").unwrap());
        client
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .await
            .unwrap();

        let url = http.last_url();
        assert_eq!(url.origin().ascii_serialization(), "http://localhost:8080");
        assert_eq!(url.path(), "/api/1.0/ttarrivals.aspx");
    }

    #[test]
    fn async_debug_hides_key() {
        let out = format!(
            "{:?}",
            AsyncClient::with_http("secret", FakeHttp::failing())
        );
        assert!(!out.contains("secret"), "{out}");
        assert!(out.contains("REDACTED"));
    }

    // ---- BlockingClient ----

    #[test]
    fn blocking_arrivals_locations_follow() {
        let client = BlockingClient::with_http("k", FakeHttp::ok(ARRIVALS));
        assert_eq!(
            client
                .arrivals(&ArrivalsQuery::at(StationId(40380)))
                .unwrap()
                .len(),
            3
        );

        let client = BlockingClient::with_http("k", FakeHttp::ok(LOCATIONS));
        assert_eq!(
            client
                .locations(&LocationsQuery::route(Route::Yellow))
                .unwrap()
                .len(),
            1
        );

        let client = BlockingClient::with_http("k", FakeHttp::ok(FOLLOW));
        assert_eq!(client.follow("125").unwrap().stops.len(), 3);
    }

    #[test]
    fn blocking_errors_map_to_variants() {
        let q = ArrivalsQuery::at(StationId(40380));

        let client = BlockingClient::with_http("k", FakeHttp::failing());
        assert!(matches!(client.arrivals(&q), Err(Error::Http(Refused))));

        let client = BlockingClient::with_http("k", FakeHttp::ok(BAD_KEY));
        assert!(matches!(
            client.arrivals(&q),
            Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[test]
    fn blocking_base_url_override() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = BlockingClient::with_http("k", http.clone())
            .base_url(Url::parse("http://localhost:8080/api/1.0/").unwrap());
        client
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .unwrap();
        assert_eq!(
            http.last_url().as_str().split('?').next(),
            Some("http://localhost:8080/api/1.0/ttarrivals.aspx")
        );
    }

    #[test]
    fn blocking_debug_hides_key() {
        let out = format!(
            "{:?}",
            BlockingClient::with_http("secret", FakeHttp::failing())
        );
        assert!(!out.contains("secret"), "{out}");
        assert!(out.contains("REDACTED"));
    }
}
