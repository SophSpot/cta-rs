[![codecov](https://codecov.io/gh/SophSpot/cta-rs/graph/badge.svg)](https://codecov.io/gh/SophSpot/cta-rs)

# cta

A Rust client for the CTA's [Train Tracker](https://www.transitchicago.com/developers/traintracker/), [Bus Tracker](https://www.transitchicago.com/developers/bustracker/), and [Customer Alerts](https://www.transitchicago.com/developers/alerts/) APIs: live arrival predictions, vehicle locations, routes, stops, detours, and service alerts for Chicago's 'L' trains and buses.

- Async (reqwest) and blocking (ureq) clients with the same API
- Typed results: IDs like `StationId`, `StopId`, and `VehicleId` are distinct types, and times are time-zone-aware
- Queries that can't be sent wrong: too many IDs, or a filter that doesn't apply, is caught before any request
- Typed errors, so you can match on "invalid key" or "run not found"
- Bring your own HTTP client, or skip HTTP entirely and use the parsers directly

This crate is not affiliated with or endorsed by the Chicago Transit Authority.

## Getting API keys

Train Tracker and Bus Tracker each need their own free key. Apply on the CTA's developer site for [Train Tracker](https://www.transitchicago.com/developers/traintracker/) or [Bus Tracker](https://www.transitchicago.com/developers/bustracker/), then pass the key to the client with `train_key` or `bus_key`. You only need the key for the API you use. The examples below read them from the `CTA_TRAIN_API_KEY` and `CTA_BUS_API_KEY` environment variables.

Customer Alerts needs no key.

## Installation

```sh
cargo add cta
```

The default `reqwest` feature gives you the async client. You'll need an async runtime such as [tokio](https://tokio.rs).

For the blocking client only:

```sh
cargo add cta --no-default-features --features ureq
```

| Feature | Default | What it enables |
|---|---|---|
| `reqwest` | yes | The default HTTP client for `AsyncClient`, backed by [reqwest](https://docs.rs/reqwest) |
| `ureq` | no | The default HTTP client for `BlockingClient`, backed by [ureq](https://docs.rs/ureq) |
| `serde` | no | `Serialize`/`Deserialize` for the response types, for caching or forwarding them |

Both HTTP clients use rustls for TLS.

## Usage

### Async

```rust
use cta::AsyncClient;
use cta::trains::{ArrivalsQuery, StationId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AsyncClient::builder()
        .train_key(std::env::var("CTA_TRAIN_API_KEY")?)
        .build();

    // Clark/Lake
    let arrivals = client.trains().arrivals(&ArrivalsQuery::at(StationId(40380))).await?;
    for a in &arrivals {
        println!("{} Line to {} in {} min", a.route, a.destination, a.eta().as_mins());
    }
    Ok(())
}
```

### Buses

```rust
use cta::AsyncClient;
use cta::buses::{Countdown, PredictionsQuery, StopId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AsyncClient::builder()
        .bus_key(std::env::var("CTA_BUS_API_KEY")?)
        .build();

    // Route 20 buses at Madison & Jefferson
    let query = PredictionsQuery::at(StopId(456)).route("20");
    for p in client.buses().predictions(&query).await? {
        match p.countdown {
            Countdown::Minutes(m) => println!("{} to {} in {m} min", p.route, p.destination),
            other => println!("{} to {}: {other:?}", p.route, p.destination),
        }
    }
    Ok(())
}
```

### Alerts

No key needed:

```rust
use cta::AsyncClient;
use cta::alerts::{AlertsQuery, RouteId};
use cta::trains::Route;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AsyncClient::builder().build();

    // Active alerts on the Brown Line and route 22
    let query = AlertsQuery::routes([RouteId::from(Route::Brown), RouteId::from("22")])?
        .active_only();
    for a in client.alerts().detailed(&query).await? {
        println!("[{}] {}", a.severity.score, a.headline);
    }
    Ok(())
}
```

### Blocking

```rust
use cta::BlockingClient;
use cta::trains::{LocationsQuery, Route};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockingClient::builder()
        .train_key(std::env::var("CTA_TRAIN_API_KEY")?)
        .build();

    for train in client.trains().locations(&LocationsQuery::route(Route::Red))? {
        println!("run {} to {}, next stop {}", train.run, train.destination, train.next_station);
    }
    Ok(())
}
```

## Keys

Train Tracker and Bus Tracker each have their own key, set on the builder with `train_key`
or `bus_key`. A client without a key is valid: calling an API whose key isn't set returns
`Error::MissingKey` without sending a request. Customer Alerts needs no key.

## Endpoints

Train Tracker methods are on `client.trains()`, for both `AsyncClient` and `BlockingClient`.

| Method | Query | Returns |
|---|---|---|
| `arrivals` | `ArrivalsQuery::at(..)`, `::stations([..])?`, or `::stops([..])?`, optionally with `.routes([..])` and `.max(..)?` | Predicted arrivals at up to 4 stations or platforms |
| `locations` | `LocationsQuery::route(..)`, `::routes([..])`, or `::all()` | Every train in service on those lines, with its next stop and position |
| `follow` | A run number, such as `arrival.run` | One train's position and its upcoming stops |

```rust
use cta::trains::{ArrivalsQuery, LocationsQuery, Route, StationId, StopId};

// Two stations in one request
let query = ArrivalsQuery::stations([StationId(40380), StationId(40360)])?;

// The next 3 Brown or Orange Line trains at one platform
let query = ArrivalsQuery::at(StopId(30074))
    .routes([Route::Brown, Route::Orange])
    .max(3)?;

// Every train on the Loop lines
let query = LocationsQuery::routes([Route::Brown, Route::Orange, Route::Pink, Route::Purple]);
```

`stations`, `stops`, and `max` return a `QueryError` for requests the CTA would reject:
no locations, more than 4 stations or stops, or `max(0)`.

Station IDs (4xxxx) cover every platform at a station, and stop IDs (3xxxx) are a single platform. Both are listed in the City of Chicago's [L stops dataset](https://data.cityofchicago.org/Transportation/CTA-System-Information-List-of-L-Stops/8pix-ypme).

Bus Tracker methods are on `client.buses()`.

| Method | Query | Returns |
|---|---|---|
| `predictions` | `PredictionsQuery::at(stop)`, `::stops([..])?` (with optional `.route(..)`), `::vehicle(..)`, or `::vehicles([..])?`; all with `.top(..)` | Buses coming to up to 10 stops, or the upcoming stops of up to 10 buses |
| `vehicles` | `VehiclesQuery::at(..)`, `::vehicles([..])?`, `::route(..)`, or `::routes([..])?` | Where buses are |
| `routes` | `RoutesQuery::all()` | Every route, with its name and color |
| `directions` | `DirectionsQuery::route(..)` | The directions a route runs in |
| `stops` | `StopsQuery::route(rt, dir)`, `::at(..)`, or `::stops([..])?` | Stops on a route in one direction, or by ID |
| `patterns` | `PatternsQuery::route(..)`, `::pattern(..)`, or `::patterns([..])?` | The paths a route takes, with stops in order |
| `detours` | `DetoursQuery::all()`, `::route(..)`, or `::route_direction(..)` | Current detours |
| `enhanced_detours` | `EnhancedDetoursQuery::all()` | Detours with trip-level details, for GTFS-Realtime feeds |
| `locales`, `time` | `LocalesQuery::all()`; none | The languages Bus Tracker accepts, and its clock |

IDs that don't exist, and stops or routes with no service right now, are left out of the results rather than reported as errors: Bus Tracker reports both the same way. Lists of IDs are capped at 10, checked before any request is sent.

Customer Alerts methods are on `client.alerts()`.

| Method | Query | Returns |
|---|---|---|
| `route_status` | `RouteStatusQuery::all()`, `::types([..])?`, `::train_routes([..])?`, `::bus_routes([..])?`, or `::station(..)` | The current status of routes, stations, and systemwide groups |
| `detailed` | `AlertsQuery::all()`, `::routes([..])?`, or `::station(..)`, with `.active_only()`, `.without_planned()`, and more | Service alerts, with severity and the services they affect |

Route status takes train and bus routes separately, because the API drops train routes from a request that mixes them. A station appears once per line serving it, and alerts for a station include alerts for its lines.

## Errors

Client methods return `cta::Error<E>`, where `E` is the HTTP client's error type:

```rust
use cta::{ApiErrorKind, Error};

match client.trains().follow(&run).await {
    Ok(train) => println!("{} stops left", train.stops.len()),
    Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound => println!("that run has ended"),
    Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey => eprintln!("check your API key"),
    Err(e) => eprintln!("{e}"),
}
```

- `Error::MissingKey`: the client has no key for that API. Nothing was sent. Never returned for Customer Alerts.
- `Error::Http`: the request failed (connection, timeout, or HTTP status).
- `Error::Api`: the CTA reported an error. `api` says which API, `kind()` names the common ones, and `message` is always available. `code` is set for Train Tracker and Customer Alerts; Bus Tracker errors only have a message. The same code means different things in different APIs, so match on `kind()`.
- `Error::Decode`: the response couldn't be read.

"Nothing matched" is an empty list, not an error: an unknown bus stop, a route with no buses running, or no current alerts.

## Times

The CTA reports Chicago local time without an offset. This crate attaches the `America/Chicago` time zone, so times like `arrival_time` are [`jiff::ZonedDateTime`](https://docs.rs/jiff) values. `jiff` is re-exported as `cta::jiff`, so you don't need a separate dependency.

`Arrival::eta()` measures from when the CTA made the prediction to the predicted arrival, using the CTA's own clock, so it doesn't depend on your machine's time.

Bus predictions can be several minutes old by the time you get them, so `Prediction::eta_from(now)` measures from a time you pass in: Bus Tracker's clock (`client.buses().time()`) or a local clock you trust. `Prediction::countdown` is the value CTA's signs show (`Due`, `Minutes(3)`, …), correct as of the response.

Alert start and end times are an `AlertTime`: an exact time when CTA gives one, or just a date when it doesn't.

## Custom HTTP clients

Pass a configured `reqwest::Client` (timeouts, proxies, a shared connection pool) to the builder:

```rust
let http = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(5))
    .build()?;
let client = cta::AsyncClient::builder().train_key(key).http(http).build();
```

To use another HTTP library, or to add logging, retries, or canned responses for tests, implement `AsyncHttpClient` or `BlockingHttpClient`:

```rust
use cta::{AsyncHttpClient, Url};

struct MyHttp(/* your client */);

impl AsyncHttpClient for MyHttp {
    type Error = std::io::Error;

    async fn get(&self, url: &Url) -> Result<Vec<u8>, Self::Error> {
        // send a GET to `url` and return the body
        todo!()
    }
}
```

Then pass it with `.http(MyHttp(..))`. The request URL contains your API key, and the
clients' `Debug` output hides it.

### Without an HTTP client

The request builders and parsers in `cta::trains`, `cta::buses`, and `cta::alerts` do no I/O. Build the URL, send it however you like, and parse the body:

```rust
use cta::Url;
use cta::trains::{ArrivalsQuery, DEFAULT_BASE_URL, StationId, arrivals_request, parse_arrivals};

let request = arrivals_request(&Url::parse(DEFAULT_BASE_URL)?, &key, &ArrivalsQuery::at(StationId(40380)));
let body: Vec<u8> = /* GET request.url */;
let arrivals = parse_arrivals(&body)?;
```

## Examples

The [`examples/`](examples) directory has runnable programs:

| Example | Shows |
|---|---|
| `arrivals` | A departure board for a station |
| `locations` | Every train running, grouped by line |
| `follow` | Picking a train and listing its upcoming stops |
| `blocking` | The blocking client |
| `custom_reqwest` | Using your own `reqwest::Client` |
| `custom_http` | Implementing `AsyncHttpClient` (runs without a key) |
| `bus_arrivals` | Upcoming buses at one or more bus stops |
| `bus_route` | A bus route's stops in order, by direction |
| `bus_detours` | Current detours, with the stops they add or skip and their patterns |
| `service_alerts` | What isn't running normally right now, and the alerts behind it (no key needed) |

```sh
CTA_TRAIN_API_KEY=... cargo run --example arrivals
CTA_BUS_API_KEY=... cargo run --example bus_arrivals -- 456
CTA_BUS_API_KEY=... cargo run --example bus_route -- 20 westbound
CTA_BUS_API_KEY=... cargo run --example bus_detours
cargo run --example service_alerts
cargo run --example custom_http
```

## Minimum supported Rust version

Rust 1.85.

## License

[MIT](LICENSE)
