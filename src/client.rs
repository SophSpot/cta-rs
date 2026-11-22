//! Async and blocking clients, and the traits for plugging in an HTTP client.

use crate::trains::{
    Arrival, ArrivalsQuery, FollowedTrain, LocationsQuery, TrainLocation, arrivals_request,
    follow_request, locations_request, parse_arrivals, parse_follow, parse_locations,
};
use crate::{Api, Error, Request, ResponseError, buses, trains};
use jiff::ZonedDateTime;
use std::fmt;
use std::marker::PhantomData;
use url::Url;

#[derive(Clone)]
struct Config {
    train_key: Option<String>,
    train_base: Url,
    bus_key: Option<String>,
    bus_base: Url,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            train_key: None,
            train_base: Url::parse(trains::DEFAULT_BASE_URL).expect("default base URL is valid"),
            bus_key: None,
            bus_base: Url::parse(buses::DEFAULT_BASE_URL).expect("default base URL is valid"),
        }
    }
}

impl Config {
    /// The Train Tracker base URL and key, or which key is missing.
    fn train(&self) -> Result<(&Url, &str), Api> {
        let key = self.train_key.as_deref().ok_or(Api::Train)?;
        Ok((&self.train_base, key))
    }

    /// The Bus Tracker base URL and key, or which key is missing.
    fn bus(&self) -> Result<(&Url, &str), Api> {
        let key = self.bus_key.as_deref().ok_or(Api::Bus)?;
        Ok((&self.bus_base, key))
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("train_key", &self.train_key.as_ref().map(|_| "REDACTED"))
            .field("train_base_url", &self.train_base.as_str())
            .field("bus_key", &self.bus_key.as_ref().map(|_| "REDACTED"))
            .field("bus_base_url", &self.bus_base.as_str())
            .finish()
    }
}

/// Add a trailing `/` so `Url::join` appends to the path instead of replacing its last segment.
fn with_trailing_slash(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}
trait Endpoint {
    type Output;
    /// Build the request, or report which API's key is missing.
    fn request(&self, config: &Config) -> Result<Request, Api>;
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError>;
}

struct ArrivalsCall<'a>(&'a ArrivalsQuery);
struct LocationsCall<'a>(&'a LocationsQuery);
struct FollowCall<'a>(&'a str);
struct TimeCall;
struct RoutesCall<'a>(&'a buses::RoutesQuery);
struct DirectionsCall<'a>(&'a buses::DirectionsQuery);
struct LocalesCall<'a>(&'a buses::LocalesQuery);
struct StopsCall<'a>(&'a buses::StopsQuery);
struct VehiclesCall<'a>(&'a buses::VehiclesQuery);
struct PredictionsCall(buses::PredictionsQuery);
struct PatternsCall<'a>(&'a buses::PatternsQuery);

impl Endpoint for ArrivalsCall<'_> {
    type Output = Vec<Arrival>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.train()?;
        Ok(arrivals_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        parse_arrivals(body)
    }
}

impl Endpoint for LocationsCall<'_> {
    type Output = Vec<TrainLocation>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.train()?;
        Ok(locations_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        parse_locations(body)
    }
}

impl Endpoint for FollowCall<'_> {
    type Output = FollowedTrain;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.train()?;
        Ok(follow_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        parse_follow(body)
    }
}

impl Endpoint for TimeCall {
    type Output = ZonedDateTime;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::time_request(base, key))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_time(body)
    }
}

impl Endpoint for RoutesCall<'_> {
    type Output = Vec<buses::RouteInfo>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::routes_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_routes(body)
    }
}

impl Endpoint for DirectionsCall<'_> {
    type Output = Vec<buses::DirectionInfo>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::directions_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_directions(body)
    }
}

impl Endpoint for StopsCall<'_> {
    type Output = Vec<buses::Stop>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::stops_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_stops(body)
    }
}

impl Endpoint for VehiclesCall<'_> {
    type Output = Vec<buses::Vehicle>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::vehicles_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_vehicles(body)
    }
}

impl Endpoint for PredictionsCall {
    type Output = Vec<buses::Prediction>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::predictions_request(base, key, &self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_predictions(body)
    }
}

impl Endpoint for PatternsCall<'_> {
    type Output = Vec<buses::Pattern>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::patterns_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_patterns(body)
    }
}

impl Endpoint for LocalesCall<'_> {
    type Output = Vec<buses::LocaleInfo>;
    fn request(&self, config: &Config) -> Result<Request, Api> {
        let (base, key) = config.bus()?;
        Ok(buses::locales_request(base, key, self.0))
    }
    fn parse(body: &[u8]) -> Result<Self::Output, ResponseError> {
        buses::parse_locales(body)
    }
}

/// Builder marker: the builder makes an [`AsyncClient`].
#[derive(Debug, Clone, Copy)]
pub struct Async;

/// Builder marker: the builder makes a [`BlockingClient`].
#[derive(Debug, Clone, Copy)]
pub struct Blocking;

/// Builder placeholder: no HTTP client set, so `build` uses the default one.
#[derive(Debug, Clone, Copy)]
pub struct DefaultHttp;

/// Builds an [`AsyncClient`] or a [`BlockingClient`].
///
/// Start from [`AsyncClient::builder`] or [`BlockingClient::builder`]. Every setting is
/// optional: a client without a key is valid, and calling an API that needs the missing
/// key returns [`Error::MissingKey`] without sending a request.
///
/// ```
/// # #[cfg(feature = "reqwest")] {
/// use cta::AsyncClient;
///
/// let client = AsyncClient::builder()
///     .train_key("your-train-tracker-key")
///     .build();
/// # }
/// ```
#[must_use]
pub struct ClientBuilder<M, H> {
    config: Config,
    http: H,
    mode: PhantomData<M>,
}

impl<M, H> fmt::Debug for ClientBuilder<M, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<M, H> ClientBuilder<M, H> {
    fn new(http: H) -> Self {
        Self {
            config: Config::default(),
            http,
            mode: PhantomData,
        }
    }

    /// The Train Tracker API key, used by [`AsyncClient::trains`] and
    /// [`BlockingClient::trains`].
    pub fn train_key(mut self, key: impl Into<String>) -> Self {
        self.config.train_key = Some(key.into());
        self
    }

    /// Send Train Tracker requests to `url` instead of the CTA, e.g. a local mock server.
    pub fn train_base_url(mut self, url: Url) -> Self {
        self.config.train_base = with_trailing_slash(url);
        self
    }

    /// The Bus Tracker API key, used by [`AsyncClient::buses`] and
    /// [`BlockingClient::buses`].
    pub fn bus_key(mut self, key: impl Into<String>) -> Self {
        self.config.bus_key = Some(key.into());
        self
    }

    /// Send Bus Tracker requests to `url` instead of the CTA, e.g. a local mock server.
    pub fn bus_base_url(mut self, url: Url) -> Self {
        self.config.bus_base = with_trailing_slash(url);
        self
    }

    /// The HTTP client to send requests with.
    pub fn http<H2>(self, http: H2) -> ClientBuilder<M, H2> {
        ClientBuilder {
            config: self.config,
            http,
            mode: PhantomData,
        }
    }
}

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
/// let client = cta::AsyncClient::builder()
///     .train_key("key")
///     .http(Canned(r#"{"ctatt":{"errCd":"0"}}"#))
///     .build();
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

impl<H> fmt::Debug for AsyncClient<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<H: AsyncHttpClient> ClientBuilder<Async, H> {
    /// Create the client.
    pub fn build(self) -> AsyncClient<H> {
        AsyncClient {
            config: self.config,
            http: self.http,
        }
    }
}

#[cfg(feature = "reqwest")]
impl ClientBuilder<Async, DefaultHttp> {
    /// Create the client with a default `reqwest::Client`.
    pub fn build(self) -> AsyncClient<reqwest::Client> {
        self.http(reqwest::Client::new()).build()
    }
}

/// An async client for the CTA APIs.
///
/// Create one with [`AsyncClient::builder`], then call an API through its view, such as
/// [`trains`](AsyncClient::trains). Cloning is cheap when the HTTP client's is, as with
/// `reqwest::Client`. The `Debug` output hides API keys.
#[derive(Clone)]
pub struct AsyncClient<H> {
    config: Config,
    http: H,
}

impl AsyncClient<()> {
    /// Start building an async client.
    pub fn builder() -> ClientBuilder<Async, DefaultHttp> {
        ClientBuilder::new(DefaultHttp)
    }
}

impl<H> AsyncClient<H> {
    /// The Train Tracker API: arrivals, train locations, and following a train.
    pub fn trains(&self) -> Trains<'_, H> {
        Trains { client: self }
    }

    /// The Bus Tracker API.
    pub fn buses(&self) -> Buses<'_, H> {
        Buses { client: self }
    }
}

impl<H: AsyncHttpClient> AsyncClient<H> {
    async fn call<E: Endpoint>(&self, endpoint: E) -> Result<E::Output, Error<H::Error>> {
        let req = endpoint
            .request(&self.config)
            .map_err(|api| Error::MissingKey { api })?;
        let body = self.http.get(&req.url).await.map_err(Error::Http)?;
        Ok(E::parse(&body)?)
    }
}

/// The Train Tracker API on an [`AsyncClient`]. Created by [`AsyncClient::trains`].
///
/// Every method needs a train key; without one it returns [`Error::MissingKey`].
pub struct Trains<'a, H> {
    client: &'a AsyncClient<H>,
}

impl<H: AsyncHttpClient> Trains<'_, H> {
    /// Predicted arrivals at a station or stop.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::trains::{ArrivalsQuery, Route, StationId};
    ///
    /// let query = ArrivalsQuery::at(StationId(40380)).route(Route::Blue).max(5)?;
    /// let arrivals = client.trains().arrivals(&query).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn arrivals(&self, query: &ArrivalsQuery) -> Result<Vec<Arrival>, Error<H::Error>> {
        self.client.call(ArrivalsCall(query)).await
    }

    /// Every train currently in service on the lines in `query`.
    pub async fn locations(
        &self,
        query: &LocationsQuery,
    ) -> Result<Vec<TrainLocation>, Error<H::Error>> {
        self.client.call(LocationsCall(query)).await
    }

    /// One train's position and its predicted arrival at each upcoming station.
    ///
    /// `run` is a run number, such as [`TrainLocation::run`] or
    /// [`Arrival::run`](crate::trains::Arrival::run). Runs end when a train reaches its
    /// terminal, so a recent run number can return
    /// [`ApiErrorKind::RunNotFound`](crate::ApiErrorKind::RunNotFound).
    pub async fn follow(&self, run: &str) -> Result<FollowedTrain, Error<H::Error>> {
        self.client.call(FollowCall(run)).await
    }
}

/// The Bus Tracker API on an [`AsyncClient`]. Created by [`AsyncClient::buses`].
///
/// Every method needs a bus key; without one it returns [`Error::MissingKey`].
pub struct Buses<'a, H> {
    client: &'a AsyncClient<H>,
}

impl<H: AsyncHttpClient> Buses<'_, H> {
    /// Bus Tracker's current time, in Chicago.
    ///
    /// Predictions and vehicle positions are timestamped by this clock, so use it rather
    /// than the local clock when comparing against them.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// let now = client.buses().time().await?;
    /// println!("Bus Tracker time: {now}");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn time(&self) -> Result<ZonedDateTime, Error<H::Error>> {
        self.client.call(TimeCall).await
    }

    /// Every bus route, with its code, name, and color.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::RoutesQuery;
    ///
    /// for r in client.buses().routes(&RoutesQuery::all()).await? {
    ///     println!("{:>4}  {}", r.route, r.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn routes(
        &self,
        query: &buses::RoutesQuery,
    ) -> Result<Vec<buses::RouteInfo>, Error<H::Error>> {
        self.client.call(RoutesCall(query)).await
    }

    /// The directions a route runs in, usually two.
    ///
    /// Some routes run in one direction only, and an unknown route has none.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::DirectionsQuery;
    ///
    /// for d in client.buses().directions(&DirectionsQuery::route("20")).await? {
    ///     println!("{}", d.direction);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn directions(
        &self,
        query: &buses::DirectionsQuery,
    ) -> Result<Vec<buses::DirectionInfo>, Error<H::Error>> {
        self.client.call(DirectionsCall(query)).await
    }

    /// Stops on a route in one direction, or stops by ID.
    ///
    /// Stops for a route come in no particular order. Stop IDs that don't exist or have
    /// no service are left out rather than reported as errors.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::{Direction, StopsQuery};
    ///
    /// let query = StopsQuery::route("20", Direction::Eastbound);
    /// for s in client.buses().stops(&query).await? {
    ///     println!("{:>5}  {}", s.id, s.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn stops(
        &self,
        query: &buses::StopsQuery,
    ) -> Result<Vec<buses::Stop>, Error<H::Error>> {
        self.client.call(StopsCall(query)).await
    }

    /// Buses in service: buses by ID, or every bus on some routes.
    ///
    /// Routes with no buses running and buses that aren't in service are left out
    /// rather than reported as errors.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::VehiclesQuery;
    ///
    /// for v in client.buses().vehicles(&VehiclesQuery::route("20")).await? {
    ///     let c = v.position.coordinates;
    ///     println!("bus {} to {} at ({:.4}, {:.4})", v.id, v.destination, c.lat, c.lon);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn vehicles(
        &self,
        query: &buses::VehiclesQuery,
    ) -> Result<Vec<buses::Vehicle>, Error<H::Error>> {
        self.client.call(VehiclesCall(query)).await
    }

    /// Predicted arrivals at some stops, or the upcoming stops of some buses.
    ///
    /// Predictions come soonest first. Stops with no service and buses that aren't in
    /// service are left out rather than reported as errors. Canceled trips are included;
    /// check [`Prediction::dynamic_action`](buses::Prediction::dynamic_action).
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::{Countdown, PredictionsQuery, StopId};
    ///
    /// let query = PredictionsQuery::at(StopId(456)).route("20").top(3);
    /// for p in client.buses().predictions(&query).await? {
    ///     match p.countdown {
    ///         Countdown::Minutes(m) => println!("{} to {} in {m} min", p.route, p.destination),
    ///         _ => println!("{} to {}: {:?}", p.route, p.destination, p.countdown),
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn predictions(
        &self,
        query: impl Into<buses::PredictionsQuery>,
    ) -> Result<Vec<buses::Prediction>, Error<H::Error>> {
        self.client.call(PredictionsCall(query.into())).await
    }

    /// The paths routes take: each pattern's stops in order, and the points for drawing
    /// it on a map.
    ///
    /// Unknown routes and pattern IDs are left out rather than reported as errors.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::{PatternPoint, PatternsQuery};
    ///
    /// for pattern in client.buses().patterns(&PatternsQuery::route("20")).await? {
    ///     println!("{} ({:.1} mi)", pattern.direction, pattern.length_ft / 5280.0);
    ///     for point in &pattern.points {
    ///         if let PatternPoint::Stop { name, .. } = point {
    ///             println!("  {name}");
    ///         }
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn patterns(
        &self,
        query: &buses::PatternsQuery,
    ) -> Result<Vec<buses::Pattern>, Error<H::Error>> {
        self.client.call(PatternsCall(query)).await
    }

    /// The languages Bus Tracker accepts, for a query's `locale` method.
    ///
    /// See [`Locale`](buses::Locale) for what CTA translates today.
    ///
    /// ```no_run
    /// # #[cfg(feature = "reqwest")]
    /// # async fn run(client: cta::AsyncClient<reqwest::Client>) -> Result<(), Box<dyn std::error::Error>> {
    /// use cta::buses::LocalesQuery;
    ///
    /// for l in client.buses().locales(&LocalesQuery::all().native_names()).await? {
    ///     println!("{}: {}", l.locale, l.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn locales(
        &self,
        query: &buses::LocalesQuery,
    ) -> Result<Vec<buses::LocaleInfo>, Error<H::Error>> {
        self.client.call(LocalesCall(query)).await
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

/// A blocking client for the CTA APIs.
///
/// The same API as [`AsyncClient`], without `async`. Create one with
/// [`BlockingClient::builder`]. The `Debug` output hides API keys.
#[derive(Clone)]
pub struct BlockingClient<H> {
    config: Config,
    http: H,
}

impl BlockingClient<()> {
    /// Start building a blocking client.
    pub fn builder() -> ClientBuilder<Blocking, DefaultHttp> {
        ClientBuilder::new(DefaultHttp)
    }
}

impl<H> BlockingClient<H> {
    /// The Train Tracker API: arrivals, train locations, and following a train.
    pub fn trains(&self) -> BlockingTrains<'_, H> {
        BlockingTrains { client: self }
    }

    /// The Bus Tracker API
    pub fn buses(&self) -> BlockingBuses<'_, H> {
        BlockingBuses { client: self }
    }
}

impl<H: BlockingHttpClient> BlockingClient<H> {
    fn call<E: Endpoint>(&self, endpoint: E) -> Result<E::Output, Error<H::Error>> {
        let req = endpoint
            .request(&self.config)
            .map_err(|api| Error::MissingKey { api })?;
        let body = self.http.get(&req.url).map_err(Error::Http)?;
        Ok(E::parse(&body)?)
    }
}

impl<H> fmt::Debug for BlockingClient<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockingClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// The Train Tracker API on a [`BlockingClient`]. Created by [`BlockingClient::trains`].
///
/// Every method needs a train key; without one it returns [`Error::MissingKey`].
pub struct BlockingTrains<'a, H> {
    client: &'a BlockingClient<H>,
}

impl<H: BlockingHttpClient> BlockingTrains<'_, H> {
    /// Predicted arrivals at the stations or stops in `query`.
    pub fn arrivals(&self, query: &ArrivalsQuery) -> Result<Vec<Arrival>, Error<H::Error>> {
        self.client.call(ArrivalsCall(query))
    }

    /// Every train currently in service on the lines in `query`.
    pub fn locations(&self, query: &LocationsQuery) -> Result<Vec<TrainLocation>, Error<H::Error>> {
        self.client.call(LocationsCall(query))
    }

    /// One train's position and its predicted arrival at each upcoming station.
    ///
    /// See [`Trains::follow`] for what `run` is and when it can fail.
    pub fn follow(&self, run: &str) -> Result<FollowedTrain, Error<H::Error>> {
        self.client.call(FollowCall(run))
    }
}

/// The Bus Tracker API on a [`BlockingClient`]. Created by [`BlockingClient::buses`].
///
/// Every method needs a bus key; without one it returns [`Error::MissingKey`].
pub struct BlockingBuses<'a, H> {
    client: &'a BlockingClient<H>,
}

impl<H: BlockingHttpClient> BlockingBuses<'_, H> {
    /// Bus Tracker's current time, in Chicago.
    ///
    /// See [`Buses::time`].
    pub fn time(&self) -> Result<ZonedDateTime, Error<H::Error>> {
        self.client.call(TimeCall)
    }

    /// Every bus route, with its code, name, and color.
    ///
    /// See [`Buses::routes`].
    pub fn routes(
        &self,
        query: &buses::RoutesQuery,
    ) -> Result<Vec<buses::RouteInfo>, Error<H::Error>> {
        self.client.call(RoutesCall(query))
    }

    /// The directions a route runs in, usually two.
    ///
    /// See [`Buses::directions`].
    pub fn directions(
        &self,
        query: &buses::DirectionsQuery,
    ) -> Result<Vec<buses::DirectionInfo>, Error<H::Error>> {
        self.client.call(DirectionsCall(query))
    }

    /// Stops on a route in one direction, or stops by ID.
    ///
    /// See [`Buses::stops`].
    pub fn stops(&self, query: &buses::StopsQuery) -> Result<Vec<buses::Stop>, Error<H::Error>> {
        self.client.call(StopsCall(query))
    }

    /// Buses in service: buses by ID, or every bus on some routes.
    ///
    /// See [`Buses::vehicles`].
    pub fn vehicles(
        &self,
        query: &buses::VehiclesQuery,
    ) -> Result<Vec<buses::Vehicle>, Error<H::Error>> {
        self.client.call(VehiclesCall(query))
    }

    /// Predicted arrivals at some stops, or the upcoming stops of some buses.
    ///
    /// See [`Buses::predictions`].
    pub fn predictions(
        &self,
        query: impl Into<buses::PredictionsQuery>,
    ) -> Result<Vec<buses::Prediction>, Error<H::Error>> {
        self.client.call(PredictionsCall(query.into()))
    }

    /// The paths routes take: each pattern's stops in order, and the points for drawing
    /// it on a map.
    ///
    /// See [`Buses::patterns`].
    pub fn patterns(
        &self,
        query: &buses::PatternsQuery,
    ) -> Result<Vec<buses::Pattern>, Error<H::Error>> {
        self.client.call(PatternsCall(query))
    }

    /// The languages Bus Tracker accepts, for a query's `locale` method.
    ///
    /// See [`Buses::locales`].
    pub fn locales(
        &self,
        query: &buses::LocalesQuery,
    ) -> Result<Vec<buses::LocaleInfo>, Error<H::Error>> {
        self.client.call(LocalesCall(query))
    }
}

impl<H: BlockingHttpClient> ClientBuilder<Blocking, H> {
    /// Create the client.
    pub fn build(self) -> BlockingClient<H> {
        BlockingClient {
            config: self.config,
            http: self.http,
        }
    }
}

#[cfg(feature = "ureq")]
impl ClientBuilder<Blocking, DefaultHttp> {
    /// Create the client with a default `ureq::Agent`.
    pub fn build(self) -> BlockingClient<ureq::Agent> {
        self.http(ureq::Agent::new_with_defaults()).build()
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
    const BUS_TIME: &[u8] = include_bytes!("../tests/fixtures/bus_time.json");
    const BUS_BAD_KEY: &[u8] = include_bytes!("../tests/fixtures/bus_bad_key.json");
    const BUS_ROUTES: &[u8] = include_bytes!("../tests/fixtures/bus_routes.json");
    const BUS_DIRECTIONS: &[u8] = include_bytes!("../tests/fixtures/bus_directions_20.json");
    const BUS_LOCALES: &[u8] = include_bytes!("../tests/fixtures/bus_locales_native.json");
    const BUS_STOPS: &[u8] = include_bytes!("../tests/fixtures/bus_stops_by_id.json");
    const BUS_VEHICLES: &[u8] = include_bytes!("../tests/fixtures/bus_vehicles_by_id.json");
    const BUS_PREDICTIONS: &[u8] = include_bytes!("../tests/fixtures/bus_predictions_456.json");
    const BUS_PATTERNS: &[u8] = include_bytes!("../tests/fixtures/bus_patterns_20.json");

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
        fn requests(&self) -> usize {
            self.seen.lock().unwrap().len()
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

    fn async_client(http: FakeHttp) -> AsyncClient<FakeHttp> {
        AsyncClient::builder().train_key("k").http(http).build()
    }

    fn blocking_client(http: FakeHttp) -> BlockingClient<FakeHttp> {
        BlockingClient::builder().train_key("k").http(http).build()
    }

    // ---- AsyncClient ----

    #[tokio::test]
    async fn async_arrivals() {
        let http = FakeHttp::ok(ARRIVALS);
        let arrivals = async_client(http.clone())
            .trains()
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .await
            .unwrap();
        assert_eq!(arrivals.len(), 3);
        assert_eq!(http.last_url().path(), "/api/1.0/ttarrivals.aspx");
        assert!(http.last_url().query().unwrap().starts_with("key=k&"));
    }

    #[tokio::test]
    async fn async_locations_and_follow() {
        let client = async_client(FakeHttp::ok(LOCATIONS));
        let trains = client
            .trains()
            .locations(&LocationsQuery::route(Route::Yellow))
            .await
            .unwrap();
        assert_eq!(trains.len(), 1);

        let client = async_client(FakeHttp::ok(FOLLOW));
        assert_eq!(client.trains().follow("125").await.unwrap().stops.len(), 3);
    }

    #[tokio::test]
    async fn async_errors_map_to_variants() {
        let q = ArrivalsQuery::at(StationId(40380));

        let client = async_client(FakeHttp::failing());
        assert!(matches!(
            client.trains().arrivals(&q).await,
            Err(Error::Http(Refused))
        ));

        let client = async_client(FakeHttp::ok(BAD_KEY));
        assert!(matches!(
            client.trains().arrivals(&q).await,
            Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));

        let client = async_client(FakeHttp::ok(b"<html>oops</html>"));
        assert!(matches!(
            client.trains().arrivals(&q).await,
            Err(Error::Decode(_))
        ));
    }

    #[tokio::test]
    async fn async_missing_key_sends_nothing() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = AsyncClient::builder().http(http.clone()).build();
        let result = client
            .trains()
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .await;
        assert!(matches!(result, Err(Error::MissingKey { api: Api::Train })));
        assert_eq!(http.requests(), 0);
    }

    #[tokio::test]
    async fn async_train_base_url_adds_trailing_slash() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = AsyncClient::builder()
            .train_key("k")
            .train_base_url(Url::parse("http://localhost:8080/api/1.0").unwrap())
            .http(http.clone())
            .build();
        client
            .trains()
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .await
            .unwrap();

        let url = http.last_url();
        assert_eq!(url.origin().ascii_serialization(), "http://localhost:8080");
        assert_eq!(url.path(), "/api/1.0/ttarrivals.aspx");
    }

    #[test]
    fn debug_hides_key() {
        let builder = AsyncClient::builder()
            .train_key("secret")
            .bus_key("secret2");
        let client = AsyncClient::builder()
            .train_key("secret")
            .bus_key("secret2")
            .http(FakeHttp::failing())
            .build();
        let blocking = BlockingClient::builder()
            .train_key("secret")
            .bus_key("secret2")
            .http(FakeHttp::failing())
            .build();
        for out in [
            format!("{builder:?}"),
            format!("{client:?}"),
            format!("{blocking:?}"),
        ] {
            assert!(!out.contains("secret"), "{out}");
            assert!(!out.contains("secret2"), "{out}");
            assert!(out.contains("REDACTED"), "{out}");
        }
    }

    #[test]
    fn debug_shows_missing_key_as_none() {
        let client = AsyncClient::builder().http(FakeHttp::failing()).build();
        let out = format!("{client:?}");
        assert!(out.contains("train_key: None"), "{out}");
        assert!(out.contains("bus_key: None"), "{out}");
    }

    #[test]
    fn bus_config_defaults_and_overrides() {
        let config = Config::default();
        assert_eq!(config.bus_base.as_str(), buses::DEFAULT_BASE_URL);
        assert_eq!(config.bus(), Err(Api::Bus));

        let builder = AsyncClient::builder()
            .bus_key("k")
            .bus_base_url(Url::parse("http://localhost:8080/bustime/api/v3").unwrap());
        assert_eq!(
            builder.config.bus_base.as_str(),
            "http://localhost:8080/bustime/api/v3/"
        );
        let (base, key) = builder.config.bus().unwrap();
        assert_eq!(key, "k");
        assert_eq!(base.as_str(), "http://localhost:8080/bustime/api/v3/");
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn async_default_http_is_reqwest() {
        let _: AsyncClient<reqwest::Client> = AsyncClient::builder().train_key("k").build();
    }

    fn async_bus_client(http: FakeHttp) -> AsyncClient<FakeHttp> {
        AsyncClient::builder().bus_key("k").http(http).build()
    }

    fn blocking_bus_client(http: FakeHttp) -> BlockingClient<FakeHttp> {
        BlockingClient::builder().bus_key("k").http(http).build()
    }

    #[tokio::test]
    async fn async_bus_time() {
        let http = FakeHttp::ok(BUS_TIME);
        let tm = async_bus_client(http.clone()).buses().time().await.unwrap();
        assert_eq!(tm.hour(), 10);
        let url = http.last_url();
        assert_eq!(url.host_str(), Some("www.ctabustracker.com"));
        assert_eq!(url.path(), "/bustime/api/v3/gettime");
        assert!(url.query().unwrap().starts_with("key=k&"));
    }

    #[tokio::test]
    async fn async_bus_routes() {
        let http = FakeHttp::ok(BUS_ROUTES);
        let routes = async_bus_client(http.clone())
            .buses()
            .routes(&buses::RoutesQuery::all())
            .await
            .unwrap();
        assert_eq!(routes.len(), 125);
        assert_eq!(http.last_url().path(), "/bustime/api/v3/getroutes");
    }

    #[tokio::test]
    async fn async_bus_directions() {
        let http = FakeHttp::ok(BUS_DIRECTIONS);
        let query = buses::DirectionsQuery::route("20").locale("es");
        let dirs = async_bus_client(http.clone())
            .buses()
            .directions(&query)
            .await
            .unwrap();
        assert_eq!(dirs.len(), 2);
        let url = http.last_url();
        assert_eq!(url.path(), "/bustime/api/v3/getdirections");
        assert_eq!(url.query(), Some("key=k&rt=20&locale=es&format=json"));
    }

    #[tokio::test]
    async fn async_bus_stops() {
        let http = FakeHttp::ok(BUS_STOPS);
        let query = buses::StopsQuery::stops([buses::StopId(456), buses::StopId(1577)]).unwrap();
        let stops = async_bus_client(http.clone())
            .buses()
            .stops(&query)
            .await
            .unwrap();
        assert_eq!(stops.len(), 1);
        let url = http.last_url();
        assert_eq!(url.path(), "/bustime/api/v3/getstops");
        assert_eq!(url.query(), Some("key=k&stpid=456%2C1577&format=json"));
    }

    #[tokio::test]
    async fn async_bus_vehicles() {
        let http = FakeHttp::ok(BUS_VEHICLES);
        let query = buses::VehiclesQuery::routes(["20", "X9"]).unwrap();
        let vehicles = async_bus_client(http.clone())
            .buses()
            .vehicles(&query)
            .await
            .unwrap();
        assert_eq!(vehicles.len(), 2);
        let url = http.last_url();
        assert_eq!(url.path(), "/bustime/api/v3/getvehicles");
        assert_eq!(url.query(), Some("key=k&rt=20%2CX9&tmres=s&format=json"));
    }

    #[tokio::test]
    async fn async_bus_predictions() {
        let http = FakeHttp::ok(BUS_PREDICTIONS);
        let query = buses::PredictionsQuery::at(buses::StopId(456))
            .route("20")
            .top(4);
        let client = async_bus_client(http.clone());
        // By reference, like the other bus methods, or by value.
        assert_eq!(client.buses().predictions(&query).await.unwrap().len(), 4);
        client.buses().predictions(query).await.unwrap();
        let url = http.last_url();
        assert_eq!(url.path(), "/bustime/api/v3/getpredictions");
        assert_eq!(
            url.query(),
            Some("key=k&stpid=456&rt=20&top=4&tmres=s&format=json")
        );
    }

    #[tokio::test]
    async fn async_bus_patterns() {
        let http = FakeHttp::ok(BUS_PATTERNS);
        let patterns = async_bus_client(http.clone())
            .buses()
            .patterns(&buses::PatternsQuery::route("20"))
            .await
            .unwrap();
        assert_eq!(patterns.len(), 2);
        let url = http.last_url();
        assert_eq!(url.path(), "/bustime/api/v3/getpatterns");
        assert_eq!(url.query(), Some("key=k&rt=20&format=json"));
    }

    #[tokio::test]
    async fn async_bus_locales() {
        let http = FakeHttp::ok(BUS_LOCALES);
        let locales = async_bus_client(http.clone())
            .buses()
            .locales(&buses::LocalesQuery::all().native_names())
            .await
            .unwrap();
        assert_eq!(locales[1].name, "español");
        let url = http.last_url();
        assert_eq!(url.path(), "/bustime/api/v3/getlocalelist");
        assert_eq!(url.query(), Some("key=k&inLocaleLanguage=true&format=json"));
    }

    #[tokio::test]
    async fn async_bus_bad_key() {
        let client = async_bus_client(FakeHttp::ok(BUS_BAD_KEY));
        assert!(matches!(
            client.buses().time().await,
            Err(Error::Api(e)) if e.api == Api::Bus && e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[tokio::test]
    async fn async_bus_missing_key_sends_nothing() {
        // A train key doesn't count as a bus key.
        let http = FakeHttp::ok(BUS_TIME);
        let client = AsyncClient::builder()
            .train_key("k")
            .http(http.clone())
            .build();
        let result = client.buses().time().await;
        assert!(matches!(result, Err(Error::MissingKey { api: Api::Bus })));
        assert_eq!(http.requests(), 0);
    }

    #[tokio::test]
    async fn async_bus_base_url() {
        let http = FakeHttp::ok(BUS_TIME);
        let client = AsyncClient::builder()
            .bus_key("k")
            .bus_base_url(Url::parse("http://localhost:8080/bustime/api/v3").unwrap())
            .http(http.clone())
            .build();
        client.buses().time().await.unwrap();
        assert_eq!(
            http.last_url().as_str().split('?').next(),
            Some("http://localhost:8080/bustime/api/v3/gettime")
        );
    }

    // ---- BlockingClient ----

    #[test]
    fn blocking_arrivals_locations_follow() {
        let client = blocking_client(FakeHttp::ok(ARRIVALS));
        let q = ArrivalsQuery::at(StationId(40380));
        assert_eq!(client.trains().arrivals(&q).unwrap().len(), 3);

        let client = blocking_client(FakeHttp::ok(LOCATIONS));
        let q = LocationsQuery::route(Route::Yellow);
        assert_eq!(client.trains().locations(&q).unwrap().len(), 1);

        let client = blocking_client(FakeHttp::ok(FOLLOW));
        assert_eq!(client.trains().follow("125").unwrap().stops.len(), 3);
    }

    #[test]
    fn blocking_errors_map_to_variants() {
        let q = ArrivalsQuery::at(StationId(40380));

        let client = blocking_client(FakeHttp::failing());
        assert!(matches!(
            client.trains().arrivals(&q),
            Err(Error::Http(Refused))
        ));

        let client = blocking_client(FakeHttp::ok(BAD_KEY));
        assert!(matches!(
            client.trains().arrivals(&q),
            Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));
    }

    #[test]
    fn blocking_bus_routes() {
        let http = FakeHttp::ok(BUS_ROUTES);
        let routes = blocking_bus_client(http.clone())
            .buses()
            .routes(&buses::RoutesQuery::all())
            .unwrap();
        assert_eq!(routes.len(), 125);
        assert_eq!(http.last_url().path(), "/bustime/api/v3/getroutes");
    }

    #[test]
    fn blocking_bus_stops() {
        let http = FakeHttp::ok(BUS_STOPS);
        let query = buses::StopsQuery::route("20", buses::Direction::Westbound);
        let stops = blocking_bus_client(http.clone())
            .buses()
            .stops(&query)
            .unwrap();
        assert_eq!(stops[0].id, buses::StopId(456));
        assert_eq!(
            http.last_url().query(),
            Some("key=k&rt=20&dir=Westbound&format=json")
        );
    }

    #[test]
    fn blocking_bus_vehicles() {
        let http = FakeHttp::ok(BUS_VEHICLES);
        let query = buses::VehiclesQuery::at(buses::VehicleId(8932));
        let vehicles = blocking_bus_client(http.clone())
            .buses()
            .vehicles(&query)
            .unwrap();
        assert_eq!(vehicles[0].id, buses::VehicleId(8932));
        assert_eq!(
            http.last_url().query(),
            Some("key=k&vid=8932&tmres=s&format=json")
        );
    }

    #[test]
    fn blocking_bus_predictions() {
        let http = FakeHttp::ok(BUS_PREDICTIONS);
        let query = buses::PredictionsQuery::vehicle(buses::VehicleId(8864));
        let preds = blocking_bus_client(http.clone())
            .buses()
            .predictions(&query)
            .unwrap();
        assert_eq!(preds.len(), 4);
        assert_eq!(
            http.last_url().query(),
            Some("key=k&vid=8864&tmres=s&format=json")
        );
    }

    #[test]
    fn blocking_bus_patterns() {
        let http = FakeHttp::ok(BUS_PATTERNS);
        let query = buses::PatternsQuery::pattern(buses::PatternId(954));
        let patterns = blocking_bus_client(http.clone())
            .buses()
            .patterns(&query)
            .unwrap();
        assert_eq!(patterns[0].id, buses::PatternId(954));
        assert_eq!(http.last_url().query(), Some("key=k&pid=954&format=json"));
    }

    #[test]
    fn blocking_bus_directions_and_locales() {
        let http = FakeHttp::ok(BUS_DIRECTIONS);
        let dirs = blocking_bus_client(http.clone())
            .buses()
            .directions(&buses::DirectionsQuery::route("20"))
            .unwrap();
        assert_eq!(dirs[0].direction, buses::Direction::Eastbound);
        assert_eq!(http.last_url().path(), "/bustime/api/v3/getdirections");

        let http = FakeHttp::ok(BUS_LOCALES);
        let locales = blocking_bus_client(http.clone())
            .buses()
            .locales(&buses::LocalesQuery::all())
            .unwrap();
        assert_eq!(locales.len(), 2);
        assert_eq!(http.last_url().path(), "/bustime/api/v3/getlocalelist");
    }

    #[test]
    fn blocking_bus_time_and_errors() {
        let client = blocking_bus_client(FakeHttp::ok(BUS_TIME));
        assert_eq!(client.buses().time().unwrap().minute(), 16);

        let client = blocking_bus_client(FakeHttp::ok(BUS_BAD_KEY));
        assert!(matches!(
            client.buses().time(),
            Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey
        ));

        let client = blocking_bus_client(FakeHttp::failing());
        assert!(matches!(client.buses().time(), Err(Error::Http(Refused))));

        let http = FakeHttp::ok(BUS_TIME);
        let client = BlockingClient::builder().http(http.clone()).build();
        assert!(matches!(
            client.buses().time(),
            Err(Error::MissingKey { api: Api::Bus })
        ));
        assert_eq!(http.requests(), 0);
    }

    #[test]
    fn blocking_missing_key_sends_nothing() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = BlockingClient::builder().http(http.clone()).build();
        let result = client.trains().follow("125");
        assert!(matches!(result, Err(Error::MissingKey { api: Api::Train })));
        assert_eq!(http.requests(), 0);
    }

    #[test]
    fn blocking_train_base_url() {
        let http = FakeHttp::ok(ARRIVALS);
        let client = BlockingClient::builder()
            .train_key("k")
            .train_base_url(Url::parse("http://localhost:8080/api/1.0/").unwrap())
            .http(http.clone())
            .build();
        client
            .trains()
            .arrivals(&ArrivalsQuery::at(StationId(40380)))
            .unwrap();
        assert_eq!(
            http.last_url().as_str().split('?').next(),
            Some("http://localhost:8080/api/1.0/ttarrivals.aspx")
        );
    }

    #[cfg(feature = "ureq")]
    #[test]
    fn blocking_default_http_is_ureq() {
        let _: BlockingClient<ureq::Agent> = BlockingClient::builder().train_key("k").build();
    }
}
