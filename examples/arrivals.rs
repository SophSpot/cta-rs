//! A departure board for up to 4 stations or platforms, fetched in one request.
//!
//! ```sh
//! CTA_TRAIN_API_KEY=... cargo run --example arrivals                          # Clark/Lake
//! CTA_TRAIN_API_KEY=... cargo run --example arrivals -- 40380 40360           # two stations
//! CTA_TRAIN_API_KEY=... cargo run --example arrivals -- --stops 30374 30375   # two platforms
//! CTA_TRAIN_API_KEY=... cargo run --example arrivals -- 40380 --routes blue,pink
//! ```
//!
//! IDs are station IDs (4xxxx) unless `--stops` is given, in which case they're
//! platform IDs (3xxxx). `--routes` keeps only trains on those lines.

use cta::AsyncClient;
use cta::trains::{Arrival, ArrivalsQuery, Route, StationId, StopId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_TRAIN_API_KEY")?;
    let args = Args::parse()?;

    // Too many IDs is a `QueryError`, caught before any request is sent.
    let query = if args.stops {
        ArrivalsQuery::stops(args.ids.iter().copied().map(StopId))?
    } else {
        ArrivalsQuery::stations(args.ids.iter().copied().map(StationId))?
    }
    .routes(args.routes);

    let client = AsyncClient::builder().train_key(key).build();
    let arrivals = client.trains().arrivals(&query).await?;

    // One board per requested ID, in the order given.
    for &id in &args.ids {
        let here: Vec<&Arrival> = arrivals
            .iter()
            .filter(|a| {
                if args.stops {
                    a.stop_id == StopId(id)
                } else {
                    a.station_id == StationId(id)
                }
            })
            .collect();

        let Some(first) = here.first() else {
            println!("No trains due at {id}.\n");
            continue;
        };
        if args.stops {
            println!("{}: {}", first.station, first.stop_description);
        } else {
            println!("{}", first.station);
        }

        for a in here {
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
                "  {:<7} {:<16} {:>7}  {}{note}",
                a.route,
                a.destination,
                when,
                a.arrival_time.strftime("%-I:%M %p"),
            );
        }
        println!();
    }
    Ok(())
}

/// Command-line arguments: IDs, plus optional `--stops` and `--routes a,b,c`.
struct Args {
    ids: Vec<u32>,
    stops: bool,
    routes: Vec<Route>,
}

impl Args {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut args = Args {
            ids: Vec::new(),
            stops: false,
            routes: Vec::new(),
        };

        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--stops" => args.stops = true,
                "--routes" => {
                    let list = iter
                        .next()
                        .ok_or("--routes needs a value, e.g. blue,pink")?;
                    args.routes = list.split(',').map(str::parse).collect::<Result<_, _>>()?;
                }
                id => args.ids.push(id.parse()?),
            }
        }

        if args.ids.is_empty() {
            if args.stops {
                return Err("--stops needs at least one platform ID".into());
            }
            args.ids.push(40380); // Clark/Lake
        }
        Ok(args)
    }
}
