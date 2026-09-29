mod auth;
mod config;
mod journey;
mod routes;
mod state;
#[cfg(test)]
mod test_support;

use std::sync::Arc;
use std::time::Duration;

use axum::http::{header::CONTENT_TYPE, HeaderName, HeaderValue, Method, Request, Response};
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::field;

use crate::{config::Config, journey::storage::R2, state::AppState};

const REQUEST_ID_HEADER: &str = "x-request-id";

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let telemetry = bikepackid_common::telemetry::init("bikepackid_journey_service");

    let config = Config::from_env();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    // No `sqlx::migrate!` here — the `journeys`/`checkpoints`/`posts`
    // schema is still owned and migrated by `auth-service`; this service only
    // ever reads/writes tables auth-service's migrations already created.
    let r2 = R2::from_config(&config);
    let journeys_cache = journey::new_journey_list_cache();

    let cors = CorsLayer::new()
        .allow_origin(
            config
                .frontend_url
                .parse::<HeaderValue>()
                .expect("FRONTEND_URL must be a valid origin"),
        )
        .allow_methods([Method::GET, Method::POST, Method::PATCH])
        .allow_headers([CONTENT_TYPE])
        .allow_credentials(true);

    let port = config.port;

    let state = Arc::new(AppState {
        db,
        config,
        r2,
        journeys_cache,
    });

    let request_id_header = HeaderName::from_static(REQUEST_ID_HEADER);

    // Same request-id/tracing layering as `auth-service` — see the comment
    // there for why the layer order matters.
    let app = routes::router()
        .with_state(state)
        .layer(cors)
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with({
                    let request_id_header = request_id_header.clone();
                    move |request: &Request<axum::body::Body>| {
                        let request_id = request
                            .headers()
                            .get(&request_id_header)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("unknown");

                        tracing::info_span!(
                            "http_request",
                            method = %request.method(),
                            path = %request.uri().path(),
                            request_id = %request_id,
                            status_code = field::Empty,
                            latency_ms = field::Empty,
                        )
                    }
                })
                .on_response(
                    |response: &Response<axum::body::Body>, latency: Duration, span: &tracing::Span| {
                        span.record("status_code", response.status().as_u16());
                        span.record("latency_ms", latency.as_millis());
                        span.in_scope(|| tracing::info!("request completed"));
                    },
                ),
        )
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind port");

    tracing::info!("bikepackid journey-service listening on port {port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");

    telemetry.shutdown();
}

/// Waits for Ctrl+C or SIGTERM (the signal a container platform sends on
/// deploy/restart) — same rationale as `auth-service`'s copy of this function.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, draining in-flight requests");
}
