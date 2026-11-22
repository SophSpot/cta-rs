//! Upcoming buses at up to 10 stops, fetched in one request.
//!
//! ```sh
//! CTA_BUS_API_KEY=... cargo run --example bus_arrivals                       # Madison & Jefferson
//! CTA_BUS_API_KEY=... cargo run --example bus_arrivals -- 456 4727           # two stops
//! CTA_BUS_API_KEY=... cargo run --example bus_arrivals -- 456 --routes 20,X20
//! ```
//!
//! IDs are bus stop IDs. Find them with `stops`, or on the sign at the stop.
//! `--routes` keeps only buses on those routes.

use cta::AsyncClient;
use cta::buses::{Countdown, Prediction, PredictionKind, PredictionsQuery, StopId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_BUS_API_KEY")?;
    let args = Args::parse()?;

    // Too many stops is a `QueryError`, caught before any request is sent.
    let query =
        PredictionsQuery::stops(args.stops.iter().copied().map(StopId))?.routes(args.routes);

    let client = AsyncClient::builder().bus_key(key).build();
    let predictions = client.buses().predictions(&query).await?;
    // Measure from Bus Tracker's clock rather than the local one.
    let now = client.buses().time().await?;

    // One board per requested stop, in the order given.
    for &id in &args.stops {
        let here: Vec<&Prediction> = predictions
            .iter()
            .filter(|p| p.stop == StopId(id))
            .collect();
        let Some(first) = here.first() else {
            println!("Stop {id}: no buses coming\n");
            continue;
        };
        println!("{} (stop {id})", first.stop_name);
        for p in here {
            let when = match &p.countdown {
                Countdown::Due => "due".to_owned(),
                Countdown::Delayed => "delayed".to_owned(),
                Countdown::Minutes(m) => format!("{m} min"),
                other => format!("{other:?}"),
            };
            let exact = p.eta_from(&now);
            let mut notes = Vec::new();
            if p.dynamic_action.is_canceled() {
                notes.push("CANCELED".to_owned());
            }
            if p.dynamic_action.is_hidden() {
                notes.push("(hidden)".to_owned());
            }
            if p.kind == PredictionKind::Departure {
                notes.push("departs".to_owned());
            }
            if let Some(bus) = p.vehicle {
                notes.push(format!("bus {bus}"));
            }
            println!(
                "  {:<4} {:<10} to {:<24} {:>8}  ({:>+4}s)  {}",
                p.route,
                p.direction,
                p.destination,
                when,
                exact.as_secs(),
                notes.join(" "),
            );
        }
        println!();
    }
    Ok(())
}

struct Args {
    stops: Vec<u32>,
    routes: Vec<String>,
}

impl Args {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut stops = Vec::new();
        let mut routes = Vec::new();
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--routes" {
                let list = args.next().ok_or("--routes needs a list, e.g. 20,X20")?;
                routes = list.split(',').map(str::to_owned).collect();
            } else {
                stops.push(arg.parse().map_err(|_| format!("not a stop ID: {arg}"))?);
            }
        }
        if stops.is_empty() {
            stops.push(456); // Madison & Jefferson
        }
        Ok(Self { stops, routes })
    }
}
