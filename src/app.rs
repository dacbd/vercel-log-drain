use crate::{handlers, types};

pub fn create_app(state: types::AppState) -> axum::Router {
    axum::Router::new()
        .route("/", axum::routing::post(handlers::root))
        .route("/health", axum::routing::get(handlers::health_check))
        .route("/vercel", axum::routing::post(handlers::ingest))
        .with_state(state)
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
        let state = types::AppState::new(
            "",
            ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, b""),
            tx,
        )?;
        let mut app = create_app(state);

        let request = Request::builder().uri("/health").body(Body::empty())?;
        let response = app.as_service().call(request).await?;

        assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }
    #[tokio::test]
    async fn root_check() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let state = types::AppState::new(
            "",
            ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, b""),
            tx,
        )?;
        let mut app = create_app(state);

        let request = Request::builder()
            .method("POST")
            .uri("/")
            .body(Body::empty())?;
        let response = app.as_service().call(request).await?;

        assert_eq!(response.status(), StatusCode::OK);
        return Ok(());
    }
    #[tokio::test]
    async fn ingest_check_samples() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let test_data = [
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
            b"deadbeef1234dacb4321",
        );

        let state = types::AppState::new("test", key.clone(), tx)?;
        let mut app = create_app(state);
        let mut app_service = app.as_service();

        for data in test_data {
            let sig = ring::hmac::sign(&key, data.as_bytes());
            let request = Request::builder()
                .method("POST")
                .header("x-vercel-signature", hex::encode(sig.as_ref()))
                .uri("/vercel")
                .body(Body::from(data))?;
            let response = app_service.call(request).await?;
            assert_eq!(
                response
                    .headers()
                    .get("x-vercel-verify")
                    .unwrap()
                    .to_str()?,
                "test"
            );
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(rx.len(), 14);
        Ok(())
    }
    #[tokio::test]
    async fn ingest_check_structured_messages() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let test_data = [
            include_str!("fixtures/structured_message_1.json"),
            include_str!("fixtures/structured_message_2.json"),
            include_str!("fixtures/sample_1.json"),
        ];

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let key = ring::hmac::Key::new(
            ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            b"deadbeef1234dacb4321",
        );

        let state = types::AppState::new("test", key.clone(), tx)?;
        let mut app = create_app(state);
        let mut app_service = app.as_service();

        for data in test_data {
            let sig = ring::hmac::sign(&key, data.as_bytes());
            let request = Request::builder()
                .method("POST")
                .header("x-vercel-signature", hex::encode(sig.as_ref()))
                .uri("/vercel")
                .body(Body::from(data))?;
            let response = app_service.call(request).await?;
            assert_eq!(
                response
                    .headers()
                    .get("x-vercel-verify")
                    .unwrap()
                    .to_str()?,
                "test"
            );
            assert_eq!(response.status(), StatusCode::OK);
        }
        assert_eq!(rx.len(), 3);
        Ok(())
    }

    /// Test Vercel's verification step.
    ///
    /// That doesn't sign the incoming request, but expects a HTTP 200 OK
    /// response and x-vercel-verify header.
    #[tokio::test]
    async fn ingest_verify_step() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let key = ring::hmac::Key::new(
            ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            b"deadbeef1234dacb4321",
        );

        let state = types::AppState::new("test", key, tx)?;
        let mut app = create_app(state);
        let mut app_service = app.as_service();
        let data = b"[]";

        let test_auth_headers = [
            // Missing header
            None,
            // Valid signature; in case Vercel fix it. :)
            Some("6e7fbac105ca8e99dd5ed29951b4443fe97f7720"),
        ];

        for auth_header in test_auth_headers {
            let mut builder = Request::builder().method("POST").uri("/vercel");

            if let Some(auth_header) = auth_header {
                builder = builder.header("x-vercel-signature", auth_header);
            }

            let request = builder.body(Body::from(data.as_ref()))?;

            let response = app_service.call(request).await?;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "auth_header: {auth_header:?}"
            );
            assert_eq!(
                response
                    .headers()
                    .get("x-vercel-verify")
                    .expect(&format!(
                        "missing x-vercel-verify header for auth_header: {auth_header:?}"
                    ))
                    .to_str()?,
                "test",
                "auth_header: {auth_header:?}"
            );
        }

        assert_eq!(rx.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn ingest_invalid_signatures() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let key = ring::hmac::Key::new(
            ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            b"deadbeef1234dacb4321",
        );

        let state = types::AppState::new("test", key, tx)?;
        let mut app = create_app(state);
        let mut app_service = app.as_service();
        let data = b"[]";

        // Everything here should fail *without* a panic.
        let test_auth_headers = [
            Some("a"),
            Some("aa"),
            Some("xx"),
            Some("000044d61091a62c339bdd0fb827afad8d61f556"),
        ];

        for auth_header in test_auth_headers {
            let mut builder = Request::builder().method("POST").uri("/vercel");

            if let Some(auth_header) = auth_header {
                builder = builder.header("x-vercel-signature", auth_header);
            }

            let request = builder.body(Body::from(data.as_ref()))?;

            let response = app_service.call(request).await?;
            assert!(
                response.headers().get("x-vercel-verify").is_none(),
                "auth_header: {auth_header:?}"
            );
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "auth_header: {auth_header:?}"
            );
        }

        assert_eq!(rx.len(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn ingest_check_json_error() -> Result<()> {
        let _ = tracing_subscriber::fmt().json().try_init();
        let test_data = [
            "{}",        // Expect an array, not an object
            "[0, 1, 2]", // Expect an array of Message
            "[",         // Unclosed tags
            "{",
        ];

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<types::Message>();
        let key = ring::hmac::Key::new(
            ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            b"deadbeef1234dacb4321",
        );

        let state = types::AppState::new("test", key.clone(), tx)?;
        let mut app = create_app(state);
        let mut app_service = app.as_service();

        for data in test_data {
            let sig = ring::hmac::sign(&key, data.as_bytes());
            let request = Request::builder()
                .method("POST")
                .header("x-vercel-signature", hex::encode(sig.as_ref()))
                .uri("/vercel")
                .body(Body::from(data))?;
            let response = app_service.call(request).await?;
            assert!(
                response.headers().get("x-vercel-verify").is_none(),
                "payload: {data:?}"
            );
            assert_eq!(
                response.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "payload: {data:?}"
            );
        }
        assert_eq!(rx.len(), 0);
        Ok(())
    }
}
