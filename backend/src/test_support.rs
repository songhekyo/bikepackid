//! Shared helpers for tests scattered across modules (`cfg(test)`-gated,
//! compiled only when running `cargo test`).

use sqlx::PgPool;
use uuid::Uuid;

pub async fn pool() -> PgPool {
    dotenvy::dotenv().ok();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run tests");
    PgPool::connect(&url)
        .await
        .expect("failed to connect to test database")
}

pub async fn insert_user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (google_id, email, name) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(format!("test-{}", Uuid::new_v4()))
    .bind(format!("{}@example.com", Uuid::new_v4()))
    .bind("Test User")
    .fetch_one(pool)
    .await
    .expect("failed to insert test user")
}

pub async fn insert_user_with_role(pool: &PgPool, role: crate::models::Role) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (google_id, email, name, role) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(format!("test-{}", Uuid::new_v4()))
    .bind(format!("{}@example.com", Uuid::new_v4()))
    .bind("Test User")
    .bind(role)
    .fetch_one(pool)
    .await
    .expect("failed to insert test user")
}

pub async fn delete_user(pool: &PgPool, user_id: Uuid) {
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
}

/// Builds a real `AppState` (real DB pool, real config from `.env`) for
/// tests that drive the app through its router rather than calling
/// individual functions directly.
pub async fn app_state() -> crate::state::SharedState {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    let db = pool().await;
    let config = crate::config::Config::from_env();
    let oauth_client = crate::auth::google::build_client(
        &config.google_client_id,
        &config.google_client_secret,
        &config.google_redirect_url,
    );

    Arc::new(crate::state::AppState {
        db,
        config,
        oauth_client,
        http_client: reqwest::Client::new(),
        pending_logins: Mutex::new(HashMap::new()),
    })
}
