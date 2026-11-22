//! A bus route's stops in order, for each direction it runs.
//!
//! ```sh
//! CTA_BUS_API_KEY=... cargo run --example bus_route                   # route 20 (Madison)
//! CTA_BUS_API_KEY=... cargo run --example bus_route -- X9
//! CTA_BUS_API_KEY=... cargo run --example bus_route -- 20 westbound
//! ```
//!
//! Stop lists from `stops` come in no particular order; patterns give them in the
//! order the bus reaches them. A route can have several patterns per direction, such as
//! short-turns and detours.

use cta::AsyncClient;
use cta::buses::{Direction, PatternPoint, PatternsQuery, Route};

const FEET_PER_MILE: f64 = 5280.0;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_BUS_API_KEY")?;
    let mut args = std::env::args().skip(1);
    let route = Route::from(args.next().unwrap_or_else(|| "20".to_owned()));
    // Text is matched ignoring case, so "westbound" works.
    let direction = args.next().map(Direction::from);

    let client = AsyncClient::builder().bus_key(key).build();
    let patterns = client
        .buses()
        .patterns(&PatternsQuery::route(route.clone()))
        .await?;

    let shown: Vec<_> = patterns
        .iter()
        .filter(|p| direction.as_ref().is_none_or(|d| &p.direction == d))
        .collect();
    if shown.is_empty() {
        match direction {
            Some(d) => println!("Route {route} has no {d} patterns"),
            None => println!("Route {route} has no patterns (is it a real route?)"),
        }
        return Ok(());
    }

    for pattern in shown {
        println!(
            "Route {route}, {} — pattern {}, {:.1} mi, {} stops",
            pattern.direction,
            pattern.id,
            pattern.length_ft / FEET_PER_MILE,
            pattern.stops().count(),
        );
        if let Some(detour) = &pattern.detour {
            println!(
                "  (detour {detour}, replaces {} points)",
                pattern.original_points.len()
            );
        }
        for point in &pattern.points {
            // Waypoints only shape the line on a map; skip them.
            if let PatternPoint::Stop {
                stop,
                name,
                distance_ft,
                ..
            } = point
            {
                println!(
                    "  {:>5.1} mi  {:<6} {name}",
                    distance_ft / FEET_PER_MILE,
                    stop.to_string()
                );
            }
        }
        println!();
    }
    Ok(())
}
