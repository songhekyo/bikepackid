mod audit;
mod auth;
mod config;
mod error;
mod journey;
mod models;
mod routes;
mod state;
#[cfg(test)]
mod test_support;
mod telemetry;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::{header::CONTENT_TYPE, HeaderName, HeaderValue, Method, Request, Response};
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::field;

use crate::{config::Config, state::AppState};

const REQUEST_ID_HEADER: &str = "x-request-id";
const SESSION_PURGE_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let telemetry = telemetry::init("bikepackid_backend");

    let config = Config::from_env();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .expect("failed to connect to database");

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("failed to run migrations");

    let oauth_client = auth::google::build_client(
        &config.google_client_id,
        &config.google_client_secret,
        &config.google_redirect_url,
    );

    // No timeout is reqwest's default, which means a slow/hanging Google
    // (or a network partition) would let a `google_callback` request hang
    // forever, tying up a connection with nothing to time it out.
    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to build HTTP client");

    let cors = CorsLayer::new()
        .allow_origin(
            config
                .frontend_url
                .parse::<HeaderValue>()
                .expect("FRONTEND_URL must be a valid origin"),
        )
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE])
        .allow_credentials(true);

    let port = config.port;

    let r2 = journey::storage::R2::from_config(&config);

    let state = Arc::new(AppState {
        db: db.clone(),
        config,
        oauth_client,
        http_client,
        pending_logins: Mutex::new(std::collections::HashMap::new()),
        r2,
        journeys_cache: journey::new_journey_list_cache(),
    });

    spawn_session_purge_task(db);

    let request_id_header = HeaderName::from_static(REQUEST_ID_HEADER);

    // Every log line and OTel span produced while handling a request
    // carries the same `request_id`, so a single field lets you pull the
    // full story for one request out of Kibana/Datadog/wherever logs land.
    // Layer order matters here: SetRequestIdLayer must be outermost (added
    // last) so the id exists before TraceLayer opens its span; Propagate
    // must be innermost so it copies the id onto the actual response.
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
                        // TraceLayer's default on_response already logs at
                        // DEBUG; overriding it (to record fields above)
                        // drops that, so log explicitly at INFO instead —
                        // this is the one line per request you'll actually
                        // see show up in Kibana/Datadog.
                        span.in_scope(|| tracing::info!("request completed"));
                    },
                ),
        )
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind port");

    tracing::info!("bikepackid backend listening on port {port}");
    // `with_connect_info` makes the raw peer address available to the rate
    // limiter as a fallback for when x-forwarded-for/x-real-ip/forwarded
    // aren't set (e.g. direct connections in local dev, no proxy).
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("server error");

    telemetry.shutdown();
}

/// Waits for Ctrl+C or SIGTERM (the signal a container platform sends on
/// deploy/restart). `axum::serve`'s graceful shutdown then lets in-flight
/// requests finish before the process exits, and — the reason this exists
/// at all — makes `telemetry.shutdown()` after `axum::serve(...).await`
/// actually reachable, instead of dead code that never runs because
/// `axum::serve` otherwise only returns on error.
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

/// Session rows are never deleted anywhere else (`revoke` just marks them
/// revoked, for the audit trail), so without this the table only grows.
/// Fire-and-forget: if the process exits mid-cycle, nothing is lost since
/// this only ever deletes rows that are already useless.
fn spawn_session_purge_task(db: sqlx::PgPool) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SESSION_PURGE_INTERVAL);
        loop {
            interval.tick().await;
            match auth::session::purge_expired(&db).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(count, "purged expired sessions"),
                Err(err) => tracing::error!(?err, "failed to purge expired sessions"),
            }
        }
    });
}
