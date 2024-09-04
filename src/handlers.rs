use crate::types;
use axum::{
    body::Bytes,
    extract::State,
    http::{header::HeaderMap, StatusCode},
    response::IntoResponse,
};
use axum_prometheus::metrics::counter;
use core::str;
use tracing::{debug, error, warn};

pub async fn root() -> impl IntoResponse {
    StatusCode::OK
}

pub async fn health_check() -> impl IntoResponse {
    StatusCode::OK
}

pub async fn ingest(
    State(state): State<types::AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    debug!("received payload");

    let Some(sig_header) = headers.get("x-vercel-signature") else {
        warn!(?headers, "received payload without signature");
        counter!("drain_recv_missing_signature").increment(1);
        return state.ok_response();
    };

    // Catch whenever we get a signature header which is not 20 bytes encoded
    // in base16.
    let Some(sig_bytes) = sig_header.to_str().ok().and_then(|sig| {
        let mut sig_bytes = [0; 20];
        hex::decode_to_slice(sig, &mut sig_bytes)
            .ok()
            .map(|()| sig_bytes)
    }) else {
        warn!(?headers, "received payload with invalid signature");
        counter!("drain_recv_invalid_signature").increment(1);
        return StatusCode::UNAUTHORIZED.into_response();
    };

    // Catch whenever the signature is invalid.
    if state.verify_signature(&body, &sig_bytes).is_err() {
        error!(?headers, "failed verifying signature");
        counter!("drain_failed_verify_signature").increment(1);
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Now that we've verified the signature, decode the payload as a UTF-8
    // string.
    let body_string = match str::from_utf8(&body) {
        Ok(body_string) => body_string,
        Err(e) => {
            error!("received bad utf-8: {e:?}");
            counter!("drain_recv_bad_utf8").increment(1);
            return StatusCode::NOT_ACCEPTABLE.into_response();
        }
    };

    // Parse the string as JSON.
    match serde_json::from_str::<types::VercelPayload>(body_string) {
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
            return StatusCode::UNPROCESSABLE_ENTITY.into_response();
        }
    }

    return state.ok_response();
}
