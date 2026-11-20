#![cfg(any(feature = "reqwest", feature = "ureq"))]
//! Live tests against the CTA API. Run with:
//! CTA_TRAIN_API_KEY=... cargo test --features reqwest,ureq -- --ignored

fn api_key() -> String {
    std::env::var("CTA_TRAIN_API_KEY").expect("set CTA_TRAIN_API_KEY to run live tests")
}

#[cfg(feature = "reqwest")]
mod with_reqwest {
    use super::api_key;
    use cta::client::AsyncClient;
    use cta::trains::{ArrivalsQuery, LocationsQuery, Route, StationId};
    use cta::{ApiErrorKind, Error};

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn arrivals_for_clark_lake() {
        let client = AsyncClient::new(api_key());

        let query = ArrivalsQuery::at(StationId(40380));
        client
            .arrivals(&query)
            .await
            .expect("request should succeed");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn bad_key_is_api_error() {
        let client = AsyncClient::with_http("not-a-real-key", reqwest::Client::new());

        let query = ArrivalsQuery::at(StationId(40380));
        let result = client.arrivals(&query).await;

        // CTA returns errCd 101 / "Invalid API key" for an unknown key.
        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey),
            "expected CTA error 101 (invalid API key), got {result:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn bad_dest_id_is_cta_error() {
        let client = AsyncClient::new(api_key());

        let query = ArrivalsQuery::at(StationId(99999999));
        let result = client.arrivals(&query).await;

        // CTA returns errCd 103 / "Invalid mapid: <id>" for an unknown station.
        assert!(
            matches!(&result, Err(Error::Api(e))
                if e.kind() == ApiErrorKind::InvalidStation && e.message.contains("99999999")),
            "expected CTA error 103 (invalid mapid), got {result:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn max_limits_result_count() {
        let client = AsyncClient::new(api_key());

        let query = ArrivalsQuery::at(StationId(40380)).max(1);
        let arrivals = client
            .arrivals(&query)
            .await
            .expect("request should succeed");

        assert!(arrivals.len() <= 1)
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn can_fetch_route_locations() {
        let client = AsyncClient::new(api_key());

        let query = LocationsQuery::routes(Route::ALL);
        client
            .locations(&query)
            .await
            .expect("request should succeed");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn can_follow_train() {
        let client = AsyncClient::new(api_key());

        let query = LocationsQuery::route(Route::Red);
        let locations = client
            .locations(&query)
            .await
            .expect("request should succeed");

        let first_train = locations
            .first()
            .expect("at least one Red Line train in service");

        client
            .follow(&first_train.run)
            .await
            .expect("follow request should succeed");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn invalid_run_number() {
        let client = AsyncClient::new(api_key());

        let result = client.follow("nope").await;

        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound),
            "expected CTA error 501 (run not found), got {result:?}"
        );
    }
}

#[cfg(feature = "ureq")]
mod with_ureq {
    use super::api_key;
    use cta::client::BlockingClient;
    use cta::trains::{ArrivalsQuery, LocationsQuery, Route, StationId};
    use cta::{ApiErrorKind, Error};

    #[test]
    #[ignore = "hits the live CTA API"]
    fn arrivals_for_clark_lake() {
        let client = BlockingClient::new(api_key());

        let query = ArrivalsQuery::at(StationId(40380));
        client.arrivals(&query).expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn bad_key_is_api_error() {
        let client = BlockingClient::with_http("not-a-real-key", ureq::Agent::new_with_defaults());

        let query = ArrivalsQuery::at(StationId(40380));
        let result = client.arrivals(&query);

        // CTA returns errCd 101 / "Invalid API key" for an unknown key.
        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey),
            "expected CTA error 101 (invalid API key), got {result:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn bad_dest_id_is_cta_error() {
        let client = BlockingClient::new(api_key());

        let query = ArrivalsQuery::at(StationId(99999999));
        let result = client.arrivals(&query);

        // CTA returns errCd 103 / "Invalid mapid: <id>" for an unknown station.
        assert!(
            matches!(&result, Err(Error::Api(e))
                if e.kind() == ApiErrorKind::InvalidStation && e.message.contains("99999999")),
            "expected CTA error 103 (invalid mapid), got {result:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn max_limits_result_count() {
        let client = BlockingClient::new(api_key());

        let query = ArrivalsQuery::at(StationId(40380)).max(1);
        let arrivals = client.arrivals(&query).expect("request should succeed");

        assert!(arrivals.len() <= 1)
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn can_fetch_route_locations() {
        let client = BlockingClient::new(api_key());

        let query = LocationsQuery::routes(Route::ALL);
        client.locations(&query).expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn can_follow_train() {
        let client = BlockingClient::new(api_key());

        let query = LocationsQuery::route(Route::Red);
        let locations = client.locations(&query).expect("request should succeed");

        let first_train = locations
            .first()
            .expect("at least one Red Line train in service");

        client
            .follow(&first_train.run)
            .expect("follow request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn invalid_run_number() {
        let client = BlockingClient::new(api_key());

        let result = client.follow("nope");

        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound),
            "expected CTA error 501 (run not found), got {result:?}"
        );
    }
}
