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
    use cta::trains::{ArrivalsQuery, LocationsQuery, Route, StationId, StopId};
    use cta::{ApiErrorKind, Error};

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn arrivals_for_clark_lake() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::at(StationId(40380));
        client
            .trains()
            .arrivals(&query)
            .await
            .expect("request should succeed");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn bad_key_is_api_error() {
        let client = AsyncClient::builder().train_key("not-a-real-key").build();

        let query = ArrivalsQuery::at(StationId(40380));
        let result = client.trains().arrivals(&query).await;

        // CTA returns errCd 101 / "Invalid API key" for an unknown key.
        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey),
            "expected CTA error 101 (invalid API key), got {result:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn bad_dest_id_is_cta_error() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::at(StationId(99999999));
        let result = client.trains().arrivals(&query).await;

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
        let client = AsyncClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::at(StationId(40380)).max(1).unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .await
            .expect("request should succeed");

        assert!(arrivals.len() <= 1)
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn can_fetch_route_locations() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let query = LocationsQuery::routes(Route::ALL);
        client
            .trains()
            .locations(&query)
            .await
            .expect("request should succeed");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn can_follow_train() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let query = LocationsQuery::route(Route::Red);
        let locations = client
            .trains()
            .locations(&query)
            .await
            .expect("request should succeed");

        let first_train = locations
            .first()
            .expect("at least one Red Line train in service");

        client
            .trains()
            .follow(&first_train.run)
            .await
            .expect("follow request should succeed");
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn invalid_run_number() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let result = client.trains().follow("nope").await;

        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound),
            "expected CTA error 501 (run not found), got {result:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn arrivals_for_several_stations() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        // Clark/Lake plus three Blue Line stations, all served around the clock.
        let ids = [40380, 40590, 40670, 40570].map(StationId);
        let query = ArrivalsQuery::stations(ids).unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .await
            .expect("4 stations should be accepted");

        assert!(
            !arrivals.is_empty(),
            "expected arrivals at 4 Blue Line stations"
        );
        assert!(
            arrivals.iter().all(|a| ids.contains(&a.station_id)),
            "got an arrival at a station that wasn't requested: {arrivals:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn arrivals_for_several_stops() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        // Both Blue Line platforms at Clark/Lake.
        let ids = [30374, 30375].map(StopId);
        let query = ArrivalsQuery::stops(ids).unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .await
            .expect("request should succeed");

        assert!(
            !arrivals.is_empty(),
            "expected Blue Line arrivals at Clark/Lake"
        );
        assert!(
            arrivals.iter().all(|a| ids.contains(&a.stop_id)),
            "got an arrival at a stop that wasn't requested: {arrivals:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn arrivals_filtered_to_several_routes() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let routes = [Route::Blue, Route::Pink];
        let query = ArrivalsQuery::at(StationId(40380)).routes(routes);
        let arrivals = client
            .trains()
            .arrivals(&query)
            .await
            .expect("request should succeed");

        assert!(
            !arrivals.is_empty(),
            "expected Blue Line arrivals at Clark/Lake"
        );
        assert!(
            arrivals.iter().all(|a| routes.contains(&a.route)),
            "got an arrival on a route that wasn't requested: {arrivals:?}"
        );
    }

    #[tokio::test]
    #[ignore = "hits the live CTA API"]
    async fn max_applies_across_several_stations() {
        let client = AsyncClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::stations([40380, 40590].map(StationId))
            .unwrap()
            .max(2)
            .unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .await
            .expect("request should succeed");

        assert!(arrivals.len() <= 2, "max(2) returned {}", arrivals.len());
    }
}

#[cfg(feature = "ureq")]
mod with_ureq {
    use super::api_key;
    use cta::client::BlockingClient;
    use cta::trains::{ArrivalsQuery, LocationsQuery, Route, StationId, StopId};
    use cta::{ApiErrorKind, Error};

    #[test]
    #[ignore = "hits the live CTA API"]
    fn arrivals_for_clark_lake() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::at(StationId(40380));
        client
            .trains()
            .arrivals(&query)
            .expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn bad_key_is_api_error() {
        let client = BlockingClient::builder()
            .train_key("not-a-real-key")
            .build();

        let query = ArrivalsQuery::at(StationId(40380));
        let result = client.trains().arrivals(&query);

        // CTA returns errCd 101 / "Invalid API key" for an unknown key.
        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::InvalidKey),
            "expected CTA error 101 (invalid API key), got {result:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn bad_dest_id_is_cta_error() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::at(StationId(99999999));
        let result = client.trains().arrivals(&query);

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
        let client = BlockingClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::at(StationId(40380)).max(1).unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .expect("request should succeed");

        assert!(arrivals.len() <= 1)
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn can_fetch_route_locations() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let query = LocationsQuery::routes(Route::ALL);
        client
            .trains()
            .locations(&query)
            .expect("request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn can_follow_train() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let query = LocationsQuery::route(Route::Red);
        let locations = client
            .trains()
            .locations(&query)
            .expect("request should succeed");

        let first_train = locations
            .first()
            .expect("at least one Red Line train in service");

        client
            .trains()
            .follow(&first_train.run)
            .expect("follow request should succeed");
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn invalid_run_number() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let result = client.trains().follow("nope");

        assert!(
            matches!(&result, Err(Error::Api(e)) if e.kind() == ApiErrorKind::RunNotFound),
            "expected CTA error 501 (run not found), got {result:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn arrivals_for_several_stations() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        // Clark/Lake plus three Blue Line stations, all served around the clock.
        let ids = [40380, 40590, 40670, 40570].map(StationId);
        let query = ArrivalsQuery::stations(ids).unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .expect("4 stations should be accepted");

        assert!(
            !arrivals.is_empty(),
            "expected arrivals at 4 Blue Line stations"
        );
        assert!(
            arrivals.iter().all(|a| ids.contains(&a.station_id)),
            "got an arrival at a station that wasn't requested: {arrivals:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn arrivals_for_several_stops() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        // Both Blue Line platforms at Clark/Lake.
        let ids = [30374, 30375].map(StopId);
        let query = ArrivalsQuery::stops(ids).unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .expect("request should succeed");

        assert!(
            !arrivals.is_empty(),
            "expected Blue Line arrivals at Clark/Lake"
        );
        assert!(
            arrivals.iter().all(|a| ids.contains(&a.stop_id)),
            "got an arrival at a stop that wasn't requested: {arrivals:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn arrivals_filtered_to_several_routes() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let routes = [Route::Blue, Route::Pink];
        let query = ArrivalsQuery::at(StationId(40380)).routes(routes);
        let arrivals = client
            .trains()
            .arrivals(&query)
            .expect("request should succeed");

        assert!(
            !arrivals.is_empty(),
            "expected Blue Line arrivals at Clark/Lake"
        );
        assert!(
            arrivals.iter().all(|a| routes.contains(&a.route)),
            "got an arrival on a route that wasn't requested: {arrivals:?}"
        );
    }

    #[test]
    #[ignore = "hits the live CTA API"]
    fn max_applies_across_several_stations() {
        let client = BlockingClient::builder().train_key(api_key()).build();

        let query = ArrivalsQuery::stations([40380, 40590].map(StationId))
            .unwrap()
            .max(2)
            .unwrap();
        let arrivals = client
            .trains()
            .arrivals(&query)
            .expect("request should succeed");

        assert!(arrivals.len() <= 2, "max(2) returned {}", arrivals.len());
    }
}
