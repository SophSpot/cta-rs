//! Pick a train on a line and list its upcoming stops.
//!
//! ```sh
//! CTA_TRAIN_API_KEY=... cargo run --example follow
//! CTA_TRAIN_API_KEY=... cargo run --example follow -- blue
//! ```

use cta::trains::{LocationsQuery, Route};
use cta::{ApiErrorKind, AsyncClient, Error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_TRAIN_API_KEY")?;
    let route: Route = std::env::args()
        .nth(1)
        .as_deref()
        .unwrap_or("red")
        .parse()?;

    let client = AsyncClient::new(key);

    let trains = client.locations(&LocationsQuery::route(route)).await?;
    let Some(train) = trains.first() else {
        println!("No {route} Line trains in service.");
        return Ok(());
    };
    println!(
        "Following {route} Line run {} to {}\n",
        train.run, train.destination
    );

    // A run can end between the two requests, when the train reaches its terminal.
    let followed = match client.follow(&train.run).await {
        Ok(followed) => followed,
        Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound => {
            println!("Run {} just finished. Try again.", train.run);
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    if let Some(p) = followed.position {
        println!(
            "Now at {:.5}, {:.5}, heading {}°\n",
            p.lat, p.lon, p.heading
        );
    }
    for stop in &followed.stops {
        println!(
            "{}  {}",
            stop.arrival_time.strftime("%-I:%M %p"),
            stop.station
        );
    }
    Ok(())
}
