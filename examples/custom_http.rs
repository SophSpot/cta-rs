//! Plug in any HTTP client by implementing `AsyncHttpClient`.
//!
//! This example needs no API key or network: it uses a fake client that returns a
//! canned response, wrapped in a client that logs each request.
//!
//! ```sh
//! cargo run --example custom_http
//! ```

use cta::trains::{ArrivalsQuery, StationId};
use cta::{AsyncClient, AsyncHttpClient, Url};

/// A fake client for tests: returns the same body for every request.
struct Canned(&'static str);

impl AsyncHttpClient for Canned {
    type Error = std::io::Error;

    async fn get(&self, _url: &Url) -> Result<Vec<u8>, Self::Error> {
        Ok(self.0.as_bytes().to_vec())
    }
}

/// Wraps any other client and logs each request. Retries, caching, and rate
/// limiting work the same way.
struct Logged<H>(H);

impl<H: AsyncHttpClient + Sync> AsyncHttpClient for Logged<H> {
    type Error = H::Error;

    async fn get(&self, url: &Url) -> Result<Vec<u8>, Self::Error> {
        // Log only the path: the full URL contains the API key.
        eprintln!("GET {}", url.path());
        let result = self.0.get(url).await;
        if let Err(e) = &result {
            eprintln!("  failed: {e}");
        }
        result
    }
}

const RESPONSE: &str = r#"{"ctatt":{"errCd":"0","errNm":null,"eta":[{"staId":"40380","stpId":"30074","staNm":"Clark/Lake","stpDe":"Service at Inner Loop platform","rn":"015","rt":"G","destSt":"30139","destNm":"Cottage Grove","trDr":"5","prdt":"2026-11-19T18:00:17","arrT":"2026-11-19T18:01:17","isApp":"1","isSch":"0","isDly":"0","isFlt":"0","flags":null}]}}"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // To wrap a client from another crate, put it in your own struct like `Logged`:
    // Rust doesn't let you implement this crate's trait directly on another crate's type.
    let client = AsyncClient::builder()
        .train_key("not-a-real-key")
        .http(Logged(Canned(RESPONSE)))
        .build();

    for a in client
        .trains()
        .arrivals(&ArrivalsQuery::at(StationId(40380)))
        .await?
    {
        println!("{} Line to {} at {}", a.route, a.destination, a.station);
    }
    Ok(())
}
