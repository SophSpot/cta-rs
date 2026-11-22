#![cfg(any(feature = "reqwest", feature = "ureq"))]
//! Live tests against the CTA Bus Tracker API. Run with:
//! CTA_BUS_API_KEY=... cargo test --features reqwest,ureq --test live_bus -- --ignored

fn api_key() -> String {
    std::env::var("CTA_BUS_API_KEY").expect("set CTA_BUS_API_KEY to run live tests")
}

#[cfg(feature = "reqwest")]
mod with_reqwest {
    use super::api_key;
    use cta::buses::{Direction, DirectionsQuery, LocalesQuery, RoutesQuery};
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
    use cta::buses::{DirectionsQuery, LocalesQuery, RoutesQuery};
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
