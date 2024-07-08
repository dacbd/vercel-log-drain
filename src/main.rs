mod controller;
mod types;

use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header::HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use clap::Parser;
use ring::hmac;
use tokio::signal::{unix, unix::SignalKind};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn, Level};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, env = "VERCEL_LOG_DRAIN_LOG_LEVEL", default_value_t = Level::INFO)]
    log: Level,
    #[arg(short, long, env = "VERCEL_LOG_DRAIN_IP", default_value = "0.0.0.0")]
    ip: String,
    #[arg(short, long, env = "VERCEL_LOG_DRAIN_PORT", default_value_t = 8000)]
    port: u16,

    #[arg(long, env = "VERCEL_VERIFY")]
    vercel_verify: String,
    #[arg(long, env = "VERCEL_SECRET")]
    vercel_secret: String,
}

#[derive(Debug, Clone)]
struct AppState {
    vercel_verify: String,
    vercel_secret: hmac::Key,
    log_queue: mpsc::UnboundedSender<types::Message>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .json()
        .with_max_level(args.log)
        .init();

    let config = aws_config::load_defaults(aws_config::BehaviorVersion::v2023_11_09()).await;

    let cwl_client = aws_sdk_cloudwatchlogs::Client::new(&config);
    let (tx, rx) = mpsc::unbounded_channel::<types::Message>();
    let mut controller = controller::Controller::new(tx.clone(), rx, cwl_client);

    controller.init_aws_state().await?;

    tokio::spawn(async move {
        controller.run().await;
    });

    let state = AppState {
        vercel_verify: args.vercel_verify,
        vercel_secret: hmac::Key::new(
            hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY,
            args.vercel_secret.as_bytes(),
        ),
        log_queue: tx,
    };

    let listen_address = format!("{}:{}", args.ip, args.port);
    let listener = tokio::net::TcpListener::bind(listen_address.clone()).await?;

    let app = axum::Router::new()
        .route("/", axum::routing::post(root))
        .route("/health", axum::routing::get(health_check))
        .route("/vercel", axum::routing::post(ingest))
        .with_state(state);

    info!("Listening on {}", listen_address);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_for_signals())
    .await?;

    Ok(())
}

async fn shutdown_for_signals() {
    tokio::select! {
        _interrupt = async {
            unix::signal(SignalKind::interrupt())
            .expect("able to listen for signals")
            .recv()
            .await
        } => {},
        _terminate = async {
        unix::signal(SignalKind::terminate())
            .expect("able to listen for signals")
            .recv()
            .await;
        } => {},
        _quit = async {
         unix::signal(SignalKind::quit())
            .expect("able to listen for signals")
            .recv()
            .await
        } => {}
    }
}

async fn root() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap()
}

async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    debug!("received payload");
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("x-vercel-verify", state.vercel_verify)
        .body(Body::empty())
        .unwrap();

    let signature = match headers.get("x-vercel-signature") {
        Some(signature) => signature.to_str().unwrap(),
        None => {
            warn!("received payload without signature");
            return response;
        }
    };
    let body_string = match String::from_utf8(body.to_vec()) {
        Ok(body_string) => body_string,
        Err(e) => {
            error!("received bad utf-8: {:?}", e);
            return response;
        }
    };
    let mut sig_bytes = [0u8; 20];
    hex::decode_to_slice(signature, &mut sig_bytes).unwrap();
    match hmac::verify(&state.vercel_secret, body_string.as_bytes(), &sig_bytes) {
        Ok(_) => {}
        Err(e) => {
            error!("failed verifying signature: {:?}", e);
            return response;
        }
    }
    match serde_json::from_str::<types::VercelPayload>(&body_string) {
        Ok(payload) => {
            debug!("parsed payload, OK");
            for message in payload.0 {
                match state.log_queue.send(message) {
                    Ok(_) => {}
                    Err(e) => {
                        error!("failed to queue log message to be sent to aws: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            error!(payload = ?body_string, "failed parsing: {:?}", e);
        }
    }
    return response;
}

async fn health_check() -> impl IntoResponse {
    return StatusCode::OK;
}
