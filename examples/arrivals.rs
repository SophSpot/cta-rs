//! A departure board for one station.
//!
//! ```sh
//! CTA_TRAIN_API_KEY=... cargo run --example arrivals          # Clark/Lake
//! CTA_TRAIN_API_KEY=... cargo run --example arrivals -- 40360 # Southport
//! ```

use cta::AsyncClient;
use cta::trains::{ArrivalsQuery, StationId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_TRAIN_API_KEY")?;
    let station = match std::env::args().nth(1) {
        Some(id) => StationId(id.parse()?),
        None => StationId(40380), // Clark/Lake
    };

    let client = AsyncClient::new(key);
    let arrivals = client.arrivals(&ArrivalsQuery::at(station)).await?;

    let Some(first) = arrivals.first() else {
        println!("No trains due at station {station}.");
        return Ok(());
    };
    println!("{}\n", first.station);

    for a in &arrivals {
        let when = if a.approaching {
            "Due".to_string()
        } else {
            format!("{} min", a.eta().as_mins())
        };
        let note = match (a.delayed, a.scheduled) {
            (true, _) => "  (delayed)",
            (_, true) => "  (scheduled)",
            _ => "",
        };
        println!(
            "{:<7} {:<16} {:>7}  {}{note}",
            a.route,
            a.destination,
            when,
            a.arrival_time.strftime("%-I:%M %p"),
        );
    }
    Ok(())
}
