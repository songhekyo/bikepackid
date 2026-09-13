mod audit;
mod auth;
mod config;
mod error;
mod models;
mod routes;
mod state;
#[cfg(test)]
mod test_support;
mod telemetry;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::{HeaderName, HeaderValue, Method, Request, Response};
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::field;

use crate::{config::Config, state::AppState};

const REQUEST_ID_HEADER: &str = "x-request-id";

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

    let cors = CorsLayer::new()
        .allow_origin(
            config
                .frontend_url
                .parse::<HeaderValue>()
                .expect("FRONTEND_URL must be a valid origin"),
        )
        .allow_methods([Method::GET, Method::POST])
        .allow_credentials(true);

    let port = config.port;

    let state = Arc::new(AppState {
        db,
        config,
        oauth_client,
        http_client: reqwest::Client::new(),
        pending_logins: Mutex::new(std::collections::HashMap::new()),
    });

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
    axum::serve(listener, app).await.expect("server error");

    telemetry.shutdown();
}
