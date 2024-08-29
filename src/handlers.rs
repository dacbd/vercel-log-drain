use crate::types;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header::HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use axum_prometheus::metrics::counter;
use ring::hmac;
use tracing::{debug, error, warn};

pub async fn root() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap()
}

pub async fn health_check() -> impl IntoResponse {
    return StatusCode::OK;
}

pub async fn ingest(
    State(state): State<types::AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    debug!("received payload");

    let signature = match headers.get("x-vercel-signature") {
        Some(signature) => signature.to_str().unwrap(),
        None => {
            warn!("received payload without signature");
            counter!("drain_recv_invalid_signature").increment(1);
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .expect("Defined Responses to be infalliable.");
        }
    };
    let body_string = match String::from_utf8(body.to_vec()) {
        Ok(body_string) => body_string,
        Err(e) => {
            error!("received bad utf-8: {:?}", e);
            counter!("drain_recv_bad_utf8").increment(1);
            return Response::builder()
                .status(StatusCode::NOT_ACCEPTABLE)
                .body(Body::empty())
                .expect("Defined Responses to be infalliable.");
        }
    };
    let mut sig_bytes = [0u8; 20];
    hex::decode_to_slice(signature, &mut sig_bytes).unwrap();
    match hmac::verify(&state.vercel_secret, body_string.as_bytes(), &sig_bytes) {
        Ok(_) => {}
        Err(e) => {
            error!("failed verifying signature: {:?}", e);
            counter!("drain_failed_verify_signature").increment(1);
            return Response::builder()
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .header("x-vercel-verify", state.vercel_verify)
                .body(Body::empty())
                .expect("Defined Responses to be infalliable.");
        }
    }
    match serde_json::from_str::<types::VercelPayload>(&body_string) {
        Ok(payload) => {
            debug!("parsed payload, OK");
            for message in payload.0 {
                match state.log_queue.send(message) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("failed to queue log message to be sent to outputs: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            error!(payload = ?body_string, "failed parsing: {:?}", e);
        }
    }
    return Response::builder()
        .status(StatusCode::OK)
        .header("x-vercel-verify", state.vercel_verify)
        .body(Body::empty())
        .expect("Defined Responses to be infalliable.");
}
