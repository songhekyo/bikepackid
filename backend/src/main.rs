mod audit;
mod auth;
mod config;
mod error;
mod models;
mod routes;
mod state;

use std::sync::{Arc, Mutex};

use axum::http::{HeaderValue, Method};
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::CorsLayer;

use crate::{config::Config, state::AppState};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

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

    let app = routes::router().with_state(state).layer(cors);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind port");

    tracing::info!("bikepackid backend listening on port {port}");
    axum::serve(listener, app).await.expect("server error");
}
