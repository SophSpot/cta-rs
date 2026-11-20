//! Every train running right now, grouped by line.
//!
//! ```sh
//! CTA_TRAIN_API_KEY=... cargo run --example locations             # all lines
//! CTA_TRAIN_API_KEY=... cargo run --example locations -- red blue
//! ```

use cta::AsyncClient;
use cta::trains::{LocationsQuery, Route};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_TRAIN_API_KEY")?;

    let routes = std::env::args()
        .skip(1)
        .map(|arg| arg.parse::<Route>())
        .collect::<Result<Vec<_>, _>>()?;
    let query = if routes.is_empty() {
        LocationsQuery::all()
    } else {
        LocationsQuery::routes(routes)
    };

    let client = AsyncClient::new(key);
    let trains = client.locations(&query).await?;

    for route in Route::ALL {
        let on_line: Vec<_> = trains.iter().filter(|t| t.route == route).collect();
        if on_line.is_empty() {
            continue;
        }

        let delayed = on_line.iter().filter(|t| t.delayed).count();
        println!("{route} Line: {} trains, {delayed} delayed", on_line.len());

        for t in on_line {
            let status = if t.approaching { "arriving at" } else { "next stop" };
            let position = t
                .position
                .map(|p| format!("({:.4}, {:.4})", p.lat, p.lon))
                .unwrap_or_default();
            println!(
                "  run {:>4} to {:<16} {status} {:<20} {position}",
                t.run, t.destination, t.next_station,
            );
        }
        println!();
    }
    Ok(())
}
