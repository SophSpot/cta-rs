//! Use your own configured `reqwest::Client`: timeouts, a user agent, proxies, or one
//! shared with the rest of your application.
//!
//! ```sh
//! CTA_TRAIN_API_KEY=... cargo run --example custom_reqwest
//! ```

use cta::AsyncClient;
use cta::trains::{ArrivalsQuery, StationId};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent("my-transit-app/1.0")
        .build()?;

    // `reqwest::Client` is cheap to clone and shares its connection pool,
    // so you can pass a clone here and keep using `http` elsewhere.
    let client = AsyncClient::builder()
        .train_key(std::env::var("CTA_TRAIN_API_KEY")?)
        .http(http.clone())
        .build();

    let arrivals = client
        .trains()
        .arrivals(&ArrivalsQuery::at(StationId(40380)))
        .await?;
    println!("{} arrivals at Clark/Lake", arrivals.len());
    Ok(())
}
