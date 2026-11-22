#![cfg(any(feature = "reqwest", feature = "ureq"))]
//! Live tests against the CTA Customer Alerts API, which needs no key. Run with:
//! cargo test --features reqwest,ureq --test live_alerts -- --ignored

#[cfg(feature = "reqwest")]
mod with_reqwest {
    use cta::alerts::{RouteStatusQuery, Service, ServiceKind};
    use cta::client::AsyncClient;
    use cta::trains::{Route, StationId};

    fn client() -> AsyncClient<reqwest::Client> {
        AsyncClient::builder().build()
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn default_route_status_is_classified() {
        let statuses = client()
            .alerts()
            .route_status(&RouteStatusQuery::all())
            .await
            .expect("request should succeed");
        assert!(statuses.len() > 100);
        let others: Vec<_> = statuses
            .iter()
            .filter(|s| matches!(s.service, Service::Other(_)))
            .collect();
        assert!(others.is_empty(), "unclassified: {others:?}");
        for line in Route::ALL {
            assert!(
                statuses
                    .iter()
                    .any(|s| s.service == Service::TrainRoute(line)),
                "missing {line}"
            );
        }
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn every_station_line_is_inferred() {
        let query = RouteStatusQuery::types([ServiceKind::Station]).unwrap();
        let stations = client()
            .alerts()
            .route_status(&query)
            .await
            .expect("request should succeed");
        let unknown: Vec<_> = stations.iter().filter(|s| s.line.is_none()).collect();
        assert!(unknown.is_empty(), "line colors changed? {unknown:?}");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn routes_and_station() {
        let alerts = client();
        let alerts = alerts.alerts();
        let lines = alerts
            .route_status(&RouteStatusQuery::train_routes([Route::Red, Route::Blue]).unwrap())
            .await
            .expect("request should succeed");
        assert_eq!(lines.len(), 2);
        let buses = alerts
            .route_status(&RouteStatusQuery::bus_routes(["20", "not-a-route"]).unwrap())
            .await
            .expect("an unknown route is left out, not an error");
        assert_eq!(buses.len(), 1);
        let clark_lake = alerts
            .route_status(&RouteStatusQuery::station(StationId(40380)))
            .await
            .expect("request should succeed");
        assert_eq!(clark_lake.len(), 6, "{clark_lake:?}");
    }
}

#[cfg(feature = "reqwest")]
mod detailed {
    use cta::alerts::{AlertsQuery, RouteId, Service, SeverityCategory};
    use cta::client::AsyncClient;
    use cta::trains::{Route, StationId};

    fn client() -> AsyncClient<reqwest::Client> {
        AsyncClient::builder().build()
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn every_affected_service_is_classified() {
        let alerts = client()
            .alerts()
            .detailed(&AlertsQuery::all())
            .await
            .expect("request should succeed");
        assert!(!alerts.is_empty());
        let others: Vec<_> = alerts
            .iter()
            .flat_map(|a| &a.services)
            .filter(|s| matches!(s.service, Service::Other(_)))
            .collect();
        assert!(others.is_empty(), "unclassified: {others:?}");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn filters_work() {
        let alerts = client();
        let alerts = alerts.alerts();
        let now = jiff::ZonedDateTime::now();
        let active = alerts
            .detailed(&AlertsQuery::all().active_only())
            .await
            .expect("request should succeed");
        for a in &active {
            assert!(
                a.start.date() <= now.date(),
                "{} starts {:?}",
                a.id,
                a.start
            );
        }
        let unplanned = alerts
            .detailed(&AlertsQuery::all().without_planned())
            .await
            .expect("request should succeed");
        assert!(
            unplanned
                .iter()
                .all(|a| a.severity.category != SeverityCategory::Planned),
            "{unplanned:?}"
        );
        let mixed = alerts
            .detailed(
                &AlertsQuery::routes([RouteId::from(Route::Brown), RouteId::from("22")]).unwrap(),
            )
            .await
            .expect("mixed train and bus routes work for alerts");
        let _ = mixed;
        let station = alerts
            .detailed(&AlertsQuery::station(StationId(40380)))
            .await
            .expect("request should succeed");
        let _ = station;
        let none = alerts
            .detailed(&AlertsQuery::routes(["not-a-route"]).unwrap())
            .await
            .expect("no matching alerts is empty, not an error");
        assert!(none.is_empty());
    }
}

#[cfg(feature = "ureq")]
mod with_ureq {
    use cta::alerts::RouteStatusQuery;
    use cta::client::BlockingClient;

    #[test]
    #[ignore = "hits the live CTA API"]
    fn route_status() {
        let client = BlockingClient::builder().build();
        let statuses = client
            .alerts()
            .route_status(&RouteStatusQuery::all())
            .expect("request should succeed");
        assert!(!statuses.is_empty());
    }
}
