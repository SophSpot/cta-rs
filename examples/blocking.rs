//! Every train on a line, using the blocking client. No async runtime needed.
//!
//! ```sh
//! CTA_TRAIN_API_KEY=... cargo run --example blocking --features ureq          # Red Line
//! CTA_TRAIN_API_KEY=... cargo run --example blocking --features ureq -- brown
//! ```

use cta::BlockingClient;
use cta::trains::{LocationsQuery, Route};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_TRAIN_API_KEY")?;
    let route: Route = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("red")
        .parse()?;

    let client = BlockingClient::new(key);
    let trains = client.locations(&LocationsQuery::route(route))?;

    println!("{} {route} Line trains in service\n", trains.len());
    for t in &trains {
        let status = if t.approaching {
            "arriving at"
        } else {
            "next stop"
        };
        println!(
            "run {:>4} to {:<16} {status} {}",
            t.run, t.destination, t.next_station
        );
    }
    Ok(())
}
