[![codecov](https://codecov.io/gh/SophSpot/cta-rs/graph/badge.svg)](https://codecov.io/gh/SophSpot/cta-rs)

# cta

A Rust client for the [CTA Train Tracker API](https://www.transitchicago.com/developers/traintracker/): live arrival predictions, train locations, and individual train runs for the Chicago 'L'.

- Async (reqwest) and blocking (ureq) clients with the same API
- Typed results: lines are a `Route` enum, IDs are `StationId`/`StopId`, and times are time-zone-aware
- Typed errors, so you can match on "invalid key" or "run not found"
- Bring your own HTTP client, or skip HTTP entirely and use the parsers directly

This crate is not affiliated with or endorsed by the Chicago Transit Authority.

## Getting an API key

The Train Tracker API needs a free key. Apply on the CTA's [developer site](https://www.transitchicago.com/developers/traintracker/), then pass the key to the client. The examples below read it from the `CTA_TRAIN_API_KEY` environment variable.

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
| `reqwest` | yes | `AsyncClient::new`, backed by [reqwest](https://docs.rs/reqwest) |
| `ureq` | no | `BlockingClient::new`, backed by [ureq](https://docs.rs/ureq) |
| `serde` | no | `Serialize`/`Deserialize` for the response types, for caching or forwarding them |

Both HTTP clients use rustls for TLS.

## Usage

### Async

```rust
use cta::AsyncClient;
use cta::trains::{ArrivalsQuery, StationId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = AsyncClient::new(std::env::var("CTA_TRAIN_API_KEY")?);

    // Clark/Lake
    let arrivals = client.arrivals(&ArrivalsQuery::at(StationId(40380))).await?;
    for a in &arrivals {
        println!("{} Line to {} in {} min", a.route, a.destination, a.eta().as_mins());
    }
    Ok(())
}
```

### Blocking

```rust
use cta::BlockingClient;
use cta::trains::{LocationsQuery, Route};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = BlockingClient::new(std::env::var("CTA_TRAIN_API_KEY")?);

    for train in client.locations(&LocationsQuery::route(Route::Red))? {
        println!("run {} to {}, next stop {}", train.run, train.destination, train.next_station);
    }
    Ok(())
}
```

## Endpoints

Each method exists on both `AsyncClient` and `BlockingClient`.

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

## Errors

Client methods return `cta::Error<E>`, where `E` is the HTTP client's error type:

```rust
use cta::{ApiErrorKind, Error};

match client.follow(&run).await {
    Ok(train) => println!("{} stops left", train.stops.len()),
    Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound => println!("that run has ended"),
    Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey => eprintln!("check your API key"),
    Err(e) => eprintln!("{e}"),
}
```

- `Error::Http`: the request failed (connection, timeout, or HTTP status).
- `Error::Api`: the CTA returned an error code. `kind()` names the common ones, and `code` and `message` are always available.
- `Error::Decode`: the response couldn't be read.

## Times

The CTA reports Chicago local time without an offset. This crate attaches the `America/Chicago` time zone, so times like `arrival_time` are [`jiff::ZonedDateTime`](https://docs.rs/jiff) values. `jiff` is re-exported as `cta::jiff`, so you don't need a separate dependency.

`Arrival::eta()` measures from when the CTA made the prediction to the predicted arrival, using the CTA's own clock, so it doesn't depend on your machine's time.

## Custom HTTP clients

Pass a configured `reqwest::Client` (timeouts, proxies, a shared connection pool) with `with_http`:

```rust
let http = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(5))
    .build()?;
let client = cta::AsyncClient::with_http(key, http);
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

The request URL contains your API key, and the clients' `Debug` output hides it.

### Without an HTTP client

The request builders and parsers in `cta::trains` do no I/O. Build the URL, send it however you like, and parse the body:

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

```sh
CTA_TRAIN_API_KEY=... cargo run --example arrivals
cargo run --example custom_http
```

## Minimum supported Rust version

Rust 1.85.

## License

[MIT](LICENSE)
