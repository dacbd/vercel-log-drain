use crate::{handlers, types};

pub fn create_app(state: types::AppState) -> axum::Router {
    return axum::Router::new()
        .route("/", axum::routing::post(handlers::root))
        .route("/health", axum::routing::get(handlers::health_check))
        .route("/vercel", axum::routing::post(handlers::ingest))
        .with_state(state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::Service;

    #[tokio::test]
    async fn health_check() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let state = types::AppState {
            vercel_verify: String::from(""),
            vercel_secret: ring::hmac::Key::new(
                ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
                "".as_bytes(),
            ),
            log_queue: tx,
        };
        let mut app = create_app(state);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let response = app.as_service().call(request).await?;

        assert_eq!(response.status(), StatusCode::OK);
        return Ok(());
    }
    #[tokio::test]
    async fn root_check() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let state = types::AppState {
            vercel_verify: String::from(""),
            vercel_secret: ring::hmac::Key::new(
                ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
                "".as_bytes(),
            ),
            log_queue: tx,
        };
        let mut app = create_app(state);

        let request = Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let response = app.as_service().call(request).await?;

        assert_eq!(response.status(), StatusCode::OK);
        return Ok(());
    }
    #[tokio::test]
    async fn ingest_check_samples() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let test_data = vec![
            include_str!("fixtures/sample_1.json"),
            include_str!("fixtures/sample_2.json"),
            include_str!("fixtures/sample_3.json"),
            include_str!("fixtures/sample_4.json"),
            include_str!("fixtures/sample_5.json"),
            // Vercel's test requests, missing projectName field
            include_str!("fixtures/test_build.json"),
            include_str!("fixtures/test_edge.json"),
            include_str!("fixtures/test_lambda.json"),
            include_str!("fixtures/test_static.json"),
        ];

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let key = ring::hmac::Key::new(
            ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            "deadbeef1234dacb4321".as_bytes(),
        );

        let state = types::AppState {
            vercel_verify: String::from("test"),
            vercel_secret: key.clone(),
            log_queue: tx,
        };
        let mut app = create_app(state);
        let mut app_service = app.as_service();

        for data in test_data {
            let sig = ring::hmac::sign(&key, data.as_bytes());
            let request = Request::builder()
                .method("POST")
                .header("x-vercel-signature", hex::encode(sig.as_ref()))
                .uri("/vercel")
                .body(Body::from(data))
                .unwrap();
            let response = app_service.call(request).await?;
            assert_eq!(
                response
                    .headers()
                    .get("x-vercel-verify")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "test"
            );
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(rx.len(), 14);
        return Ok(());
    }
    #[tokio::test]
    async fn ingest_check_structured_messages() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let test_data = vec![
            include_str!("fixtures/structured_message_1.json"),
            include_str!("fixtures/structured_message_2.json"),
            include_str!("fixtures/sample_1.json"),
        ];

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let key = ring::hmac::Key::new(
            ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            "deadbeef1234dacb4321".as_bytes(),
        );

        let state = types::AppState {
            vercel_verify: String::from("test"),
            vercel_secret: key.clone(),
            log_queue: tx,
        };
        let mut app = create_app(state);
        let mut app_service = app.as_service();

        for data in test_data {
            let sig = ring::hmac::sign(&key, data.as_bytes());
            let request = Request::builder()
                .method("POST")
                .header("x-vercel-signature", hex::encode(sig.as_ref()))
                .uri("/vercel")
                .body(Body::from(data))
                .unwrap();
            let response = app_service.call(request).await?;
            assert_eq!(
                response
                    .headers()
                    .get("x-vercel-verify")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "test"
            );
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(rx.len(), 3);
        return Ok(());
    }
}
