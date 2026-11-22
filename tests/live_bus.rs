#![cfg(any(feature = "reqwest", feature = "ureq"))]
//! Live tests against the CTA Bus Tracker API. Run with:
//! CTA_BUS_API_KEY=... cargo test --features reqwest,ureq --test live_bus -- --ignored

fn api_key() -> String {
    std::env::var("CTA_BUS_API_KEY").expect("set CTA_BUS_API_KEY to run live tests")
}

#[cfg(feature = "reqwest")]
mod with_reqwest {
    use super::api_key;
    use cta::buses::{
        DetoursQuery, Direction, DirectionsQuery, EnhancedDetoursQuery, LocalesQuery, PatternId,
        PatternsQuery, PredictionsQuery, RoutesQuery, StopId, StopsQuery, VehicleId, VehiclesQuery,
    };
    use cta::client::AsyncClient;
    use cta::{Api, ApiErrorKind, Error};

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn time() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let tm = client.buses().time().await.expect("request should succeed");
        let drift = jiff::ZonedDateTime::now().duration_since(&tm).abs();
        assert!(
            drift < jiff::SignedDuration::from_mins(5),
            "{tm} is {drift:?} off"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn routes() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let routes = client
            .buses()
            .routes(&RoutesQuery::all())
            .await
            .expect("request should succeed");
        assert!(
            routes.iter().any(|r| r.route.as_str() == "20"),
            "route 20 (Madison) should exist: {routes:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn directions() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let dirs = client
            .buses()
            .directions(&DirectionsQuery::route("20"))
            .await
            .expect("request should succeed");
        let dirs: Vec<_> = dirs.into_iter().map(|d| d.direction).collect();
        assert_eq!(dirs, [Direction::Eastbound, Direction::Westbound]);
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn unknown_route_has_no_directions() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let dirs = client
            .buses()
            .directions(&DirectionsQuery::route("not-a-route"))
            .await
            .expect("an unknown route is empty, not an error");
        assert!(dirs.is_empty(), "{dirs:?}");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn stops_for_route() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let stops = client
            .buses()
            .stops(&StopsQuery::route("20", Direction::Westbound))
            .await
            .expect("request should succeed");
        assert!(
            stops.iter().any(|s| s.id == StopId(456)),
            "Madison & Jefferson should be on westbound 20: {stops:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn unknown_stop_ids_are_left_out() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let query = StopsQuery::stops([StopId(456), StopId(999_999)]).unwrap();
        let stops = client
            .buses()
            .stops(&query)
            .await
            .expect("an unknown stop is left out, not an error");
        let ids: Vec<_> = stops.iter().map(|s| s.id).collect();
        assert_eq!(ids, [StopId(456)]);
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn vehicles_by_route_then_by_id() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        // Some of these will have buses at any hour.
        let query = VehiclesQuery::routes(["3", "4", "9", "20", "22", "49", "79", "N5"]).unwrap();
        let vehicles = client
            .buses()
            .vehicles(&query)
            .await
            .expect("request should succeed");
        let Some(first) = vehicles.first() else {
            return; // nothing running; nothing more to check
        };
        let age = jiff::ZonedDateTime::now().duration_since(&first.updated_at);
        assert!(
            age < jiff::SignedDuration::from_mins(10),
            "stale: {first:?}"
        );

        let query = VehiclesQuery::vehicles([first.id, VehicleId(999_999)]).unwrap();
        let again = client
            .buses()
            .vehicles(&query)
            .await
            .expect("an unknown bus is left out, not an error");
        let ids: Vec<_> = again.iter().map(|v| v.id).collect();
        assert_eq!(ids, [first.id]);
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn predictions_for_stop_then_vehicle() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        // Madison & Jefferson (westbound 20), plus a stop that doesn't exist.
        let query = PredictionsQuery::stops([StopId(456), StopId(999_999)])
            .unwrap()
            .route("20")
            .top(3);
        let preds = client
            .buses()
            .predictions(&query)
            .await
            .expect("an unknown stop is left out, not an error");
        assert!(preds.len() <= 3, "{preds:?}");
        assert!(preds.iter().all(|p| p.stop == StopId(456)));
        assert!(preds.iter().all(|p| p.route.as_str() == "20"));

        let now = client.buses().time().await.expect("time should work");
        if let Some(p) = preds.iter().find(|p| !p.dynamic_action.is_canceled()) {
            let eta = p.eta_from(&now);
            assert!(
                eta < jiff::SignedDuration::from_hours(3),
                "{eta:?} for {p:?}"
            );
        }

        let Some(bus) = preds.iter().find_map(|p| p.vehicle) else {
            return; // nothing running; nothing more to check
        };
        let upcoming = client
            .buses()
            .predictions(PredictionsQuery::vehicle(bus))
            .await
            .expect("request should succeed");
        assert!(upcoming.iter().all(|p| p.vehicle == Some(bus)));
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn patterns_for_route_then_by_id() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let patterns = client
            .buses()
            .patterns(&PatternsQuery::route("20"))
            .await
            .expect("request should succeed");
        assert!(!patterns.is_empty());
        for p in &patterns {
            assert!(p.stops().count() > 10, "{:?} has few stops", p.id);
            assert!(p.length_ft > 10_000.0);
        }
        // Madison & Jefferson is on a westbound pattern.
        assert!(patterns.iter().any(|p| {
            p.direction == Direction::Westbound && p.stops().any(|s| s == StopId(456))
        }));

        let ids = [patterns[0].id, PatternId(1)];
        let again = client
            .buses()
            .patterns(&PatternsQuery::patterns(ids).unwrap())
            .await
            .expect("an unknown pattern is left out, not an error");
        assert_eq!(again.len(), 1);
        assert_eq!(again[0], patterns[0]);
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn detours_link_to_patterns() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let detours = client
            .buses()
            .detours(&DetoursQuery::all())
            .await
            .expect("request should succeed");
        let Some(detour) = detours.iter().find(|d| d.active && !d.affects.is_empty()) else {
            return; // no active detours today; nothing more to check
        };
        let affected = &detour.affects[0];

        // Filtering by route and direction finds the same detour.
        let query =
            DetoursQuery::route_direction(affected.route.clone(), affected.direction.clone());
        let filtered = client
            .buses()
            .detours(&query)
            .await
            .expect("request should succeed");
        assert!(filtered.iter().any(|d| d.id == detour.id), "{filtered:?}");

        // The route's patterns usually include the detour's pattern.
        let patterns = client
            .buses()
            .patterns(&PatternsQuery::route(affected.route.clone()))
            .await
            .expect("request should succeed");
        if let Some(p) = patterns
            .iter()
            .find(|p| p.detour.as_ref() == Some(&detour.id))
        {
            assert!(!p.original_points.is_empty());
        }
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn enhanced_detours_match_detours_and_patterns() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let schedules = client
            .buses()
            .enhanced_detours(&EnhancedDetoursQuery::all())
            .await
            .expect("request should succeed");
        let detours = client
            .buses()
            .detours(&DetoursQuery::all())
            .await
            .expect("request should succeed");
        // Enhanced detours also include upcoming ones, so every current detour is there.
        for d in &detours {
            assert!(
                schedules.iter().any(|s| s.detour.id == d.id),
                "{} missing from enhanced detours",
                d.id
            );
        }
        for s in &schedules {
            for p in &s.patterns {
                let path = p.decode_path().expect("paths should decode");
                assert!(
                    path.iter()
                        .all(|c| (41.0..43.0).contains(&c.lat) && (-88.5..-87.0).contains(&c.lon)),
                    "path for {:?} isn't in Chicago",
                    p.detour
                );
            }
        }
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn unknown_route_has_no_detours() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let detours = client
            .buses()
            .detours(&DetoursQuery::route("not-a-route"))
            .await
            .expect("an unknown route is empty, not an error");
        assert!(detours.is_empty());
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn locales() {
        let client = AsyncClient::builder().bus_key(api_key()).build();
        let locales = client
            .buses()
            .locales(&LocalesQuery::all().native_names())
            .await
            .expect("request should succeed");
        assert!(
            locales
                .iter()
                .any(|l| l.locale.as_str() == "es" && l.name == "español"),
            "{locales:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn bad_key_is_api_error() {
        let client = AsyncClient::builder().bus_key("not-a-real-key").build();
        let result = client.buses().time().await;
        assert!(
            matches!(&result, Err(Error::Api(e))
                if e.api == Api::Bus && e.kind() == ApiErrorKind::InvalidKey),
            "expected an invalid key error, got {result:?}"
        );
    }
}

#[cfg(feature = "ureq")]
mod with_ureq {
    use super::api_key;
    use cta::buses::{
        DetoursQuery, DirectionsQuery, LocalesQuery, PatternsQuery, PredictionsQuery, RoutesQuery,
        StopId, StopsQuery, VehiclesQuery,
    };
    use cta::client::BlockingClient;
    use cta::{ApiErrorKind, Error};

    #[test]
    #[ignore = "hits the live CTA API"]
    fn time() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        client.buses().time().expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn routes() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        let routes = client
            .buses()
            .routes(&RoutesQuery::all())
            .expect("request should succeed");
        assert!(!routes.is_empty());
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn stops() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        let stops = client
            .buses()
            .stops(&StopsQuery::at(StopId(456)))
            .expect("request should succeed");
        assert_eq!(stops.len(), 1);
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn vehicles() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        client
            .buses()
            .vehicles(&VehiclesQuery::route("20"))
            .expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn predictions() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        let query = PredictionsQuery::at(StopId(456)).top(2);
        let preds = client
            .buses()
            .predictions(&query)
            .expect("request should succeed");
        assert!(preds.len() <= 2);
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn patterns() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        let patterns = client
            .buses()
            .patterns(&PatternsQuery::route("19"))
            .expect("request should succeed");
        assert!(patterns.iter().all(|p| p.stops().count() > 0));
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn detours() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        client
            .buses()
            .detours(&DetoursQuery::all())
            .expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn directions_and_locales() {
        let client = BlockingClient::builder().bus_key(api_key()).build();
        let dirs = client
            .buses()
            .directions(&DirectionsQuery::route("19"))
            .expect("request should succeed");
        assert!(!dirs.is_empty());
        let locales = client
            .buses()
            .locales(&LocalesQuery::all())
            .expect("request should succeed");
        assert!(locales.iter().any(|l| l.locale.as_str() == "en"));
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn bad_key_is_api_error() {
        let client = BlockingClient::builder().bus_key("not-a-real-key").build();
        let result = client.buses().time();
        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey),
            "expected an invalid key error, got {result:?}"
        );
    }
}
