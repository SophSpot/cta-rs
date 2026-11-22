//! Current bus detours: which routes they affect, which stops they add or skip, and
//! the detour patterns buses follow.
//!
//! ```sh
//! CTA_BUS_API_KEY=... cargo run --example bus_detours          # every detour
//! CTA_BUS_API_KEY=... cargo run --example bus_detours -- 49    # one route's
//! ```
//!
//! This follows the links described in the `buses` module docs: each `Detour` lists the
//! routes and directions it affects, stops carry the IDs of detours that add or remove
//! them, and detour patterns carry the ID of the detour that created them.

use cta::AsyncClient;
use cta::buses::{DetoursQuery, Direction, Pattern, PatternsQuery, Route, Stop, StopsQuery};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("CTA_BUS_API_KEY")?;
    let query = match std::env::args().nth(1) {
        Some(route) => DetoursQuery::route(route),
        None => DetoursQuery::all(),
    };

    let client = AsyncClient::builder().bus_key(key).build();
    let detours = client.buses().detours(&query).await?;
    if detours.is_empty() {
        println!("No detours");
        return Ok(());
    }

    // Several detours can share a route, so fetch each route's stops and patterns once.
    let mut stops: HashMap<(Route, Direction), Vec<Stop>> = HashMap::new();
    let mut patterns: HashMap<Route, Vec<Pattern>> = HashMap::new();

    for detour in &detours {
        let state = if detour.active { "" } else { " CANCELED" };
        println!(
            "Detour {} (version {}){state}: {}",
            detour.id, detour.version, detour.description
        );
        println!(
            "  {} to {}",
            detour.start.strftime("%b %d %H:%M"),
            detour.end.strftime("%b %d %H:%M")
        );

        for affected in &detour.affects {
            println!("  Route {} {}", affected.route, affected.direction);

            let key = (affected.route.clone(), affected.direction.clone());
            if !stops.contains_key(&key) {
                let query = StopsQuery::route(affected.route.clone(), affected.direction.clone());
                stops.insert(key.clone(), client.buses().stops(&query).await?);
            }
            let route_stops = &stops[&key];
            let added: Vec<&Stop> = route_stops
                .iter()
                .filter(|s| s.detours_added.contains(&detour.id))
                .collect();
            let skipped: Vec<&Stop> = route_stops
                .iter()
                .filter(|s| s.detours_removed.contains(&detour.id))
                .collect();
            print_stops("adds", &added);
            print_stops("skips", &skipped);

            if !patterns.contains_key(&affected.route) {
                let query = PatternsQuery::route(affected.route.clone());
                patterns.insert(
                    affected.route.clone(),
                    client.buses().patterns(&query).await?,
                );
            }
            for p in patterns[&affected.route]
                .iter()
                .filter(|p| p.detour.as_ref() == Some(&detour.id))
            {
                println!(
                    "    pattern {}: {} stops, {:.1} mi, replaces {} points of the usual path",
                    p.id,
                    p.stops().count(),
                    p.length_ft / 5280.0,
                    p.original_points.len()
                );
            }
        }
        println!();
    }
    Ok(())
}

fn print_stops(verb: &str, stops: &[&Stop]) {
    if stops.is_empty() {
        return;
    }
    println!("    {verb} {} stops:", stops.len());
    for s in stops {
        println!("      {:<6} {}", s.id.to_string(), s.name);
    }
}
