//! What isn't running normally on the CTA right now, and why. Needs no API key.
//!
//! ```sh
//! cargo run --example service_alerts            # every route not running normally
//! cargo run --example service_alerts -- 40380   # one station (Clark/Lake), per line
//! ```
//!
//! Statuses come from `route_status`; the alerts behind them come from `detailed`.

use cta::AsyncClient;
use cta::alerts::{Alert, AlertTime, AlertsQuery, RouteStatus, RouteStatusQuery, Service};
use cta::trains::StationId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Customer Alerts needs no key.
    let client = AsyncClient::builder().build();
    let alerts = client.alerts();

    match std::env::args().nth(1) {
        Some(id) => {
            let station = StationId(id.parse().map_err(|_| format!("not a station ID: {id}"))?);
            let statuses = alerts
                .route_status(&RouteStatusQuery::station(station))
                .await?;
            let active = alerts
                .detailed(&AlertsQuery::station(station).active_only())
                .await?;
            show_station(station, &statuses, &active);
        }
        None => {
            let statuses = alerts.route_status(&RouteStatusQuery::all()).await?;
            let active = alerts.detailed(&AlertsQuery::all().active_only()).await?;
            show_system(&statuses, &active);
        }
    }
    Ok(())
}

/// A section heading and which services belong under it.
type Group = (&'static str, fn(&Service) -> bool);

const GROUPS: [Group; 3] = [
    ("Trains", |s| matches!(s, Service::TrainRoute(_))),
    ("Buses", |s| matches!(s, Service::BusRoute(_))),
    ("Systemwide", |s| {
        matches!(s, Service::Systemwide(_) | Service::Other(_))
    }),
];

fn show_system(statuses: &[RouteStatus], active: &[Alert]) {
    let not_normal: Vec<&RouteStatus> = statuses.iter().filter(|s| !s.is_normal()).collect();
    if not_normal.is_empty() {
        println!("Everything is running normally.");
        return;
    }
    for (title, in_group) in GROUPS {
        let here: Vec<&RouteStatus> = not_normal
            .iter()
            .copied()
            .filter(|s| in_group(&s.service))
            .collect();
        if here.is_empty() {
            continue;
        }
        println!("{title}");
        for s in here {
            let label = match &s.service {
                Service::BusRoute(r) => format!("#{r} {}", s.name),
                _ => s.name.clone(),
            };
            println!("  {label}: {}", s.status);
            show_alerts(&s.service, active, "    ");
        }
        println!();
    }
}

fn show_station(station: StationId, statuses: &[RouteStatus], active: &[Alert]) {
    let Some(first) = statuses.first() else {
        println!("No station {station}");
        return;
    };
    println!("{} ({station})", first.name);
    if let Some(address) = &first.address {
        println!("  {address}");
    }
    // A station appears once per line serving it.
    for s in statuses {
        let line = s
            .line
            .map_or_else(|| "?".to_owned(), |l| format!("{l} Line"));
        println!("  {line:<14} {}", s.status);
    }

    // Alerts for a station also include alerts for every line serving it.
    let here = Service::Station(station);
    println!("\nAt this station:");
    let at_station = active
        .iter()
        .filter(|a| a.services.iter().any(|s| s.service == here));
    print_sorted(at_station, "  ");
    for s in statuses {
        let Some(line) = s.line else { continue };
        let on_line = Service::TrainRoute(line);
        let line_alerts = active.iter().filter(|a| {
            a.services.iter().all(|s| s.service != here)
                && a.services.iter().any(|s| s.service == on_line)
        });
        let line_alerts: Vec<&Alert> = line_alerts.collect();
        if !line_alerts.is_empty() {
            println!("\nElsewhere on the {line} Line:");
            print_sorted(line_alerts, "  ");
        }
    }
}

/// Print alerts, most severe first, or "none".
fn print_sorted<'a>(alerts: impl IntoIterator<Item = &'a Alert>, indent: &str) {
    let mut alerts: Vec<&Alert> = alerts.into_iter().collect();
    if alerts.is_empty() {
        println!("{indent}none");
    }
    alerts.sort_by_key(|a| std::cmp::Reverse(a.severity.score));
    for a in alerts {
        print_alert(a, indent);
    }
}

/// The active alerts that affect `service`, most severe first.
fn show_alerts(service: &Service, active: &[Alert], indent: &str) {
    let matching = active
        .iter()
        .filter(|a| a.services.iter().any(|s| &s.service == service));
    print_sorted(matching, indent);
}

fn print_alert(a: &Alert, indent: &str) {
    let until = match &a.end {
        None => "until further notice".to_owned(),
        Some(end) => format!("until {}", when(end)),
    };
    println!(
        "{indent}- {} [{:?}, {}] {until}",
        a.headline, a.severity.category, a.severity.score
    );
}

fn when(t: &AlertTime) -> String {
    match t.time() {
        Some(time) => time.strftime("%b %-d %-I:%M%P").to_string(),
        None => t.date().strftime("%b %-d").to_string(),
    }
}
