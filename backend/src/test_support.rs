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

pub async fn fetch_user(pool: &PgPool, user_id: Uuid) -> crate::models::User {
    sqlx::query_as::<_, crate::models::User>("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("failed to fetch test user")
}

/// Inserts a journey directly at a given status, sidestepping
/// `journey::service::create_journey` (which always starts at `draft`) —
/// useful for tests that need a `planning`/`published`/`archived` journey
/// without going through a status-transition call first. Always fills in
/// start/end lat-lng so the `CHECK` constraint is satisfied regardless of
/// status.
pub async fn insert_journey(pool: &PgPool, user_id: Uuid, status: &str) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO journeys (user_id, title, status, start_lat, start_lng, end_lat, end_lng)
        VALUES ($1, $2, $3::journey_status, $4, $5, $6, $7)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(format!("Test journey {}", Uuid::new_v4()))
    .bind(status)
    .bind(1.0_f64)
    .bind(2.0_f64)
    .bind(3.0_f64)
    .bind(4.0_f64)
    .fetch_one(pool)
    .await
    .expect("failed to insert test journey")
}

pub async fn insert_checkpoint(pool: &PgPool, journey_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO checkpoints (journey_id, lat, lng, captured_at)
        VALUES ($1, $2, $3, now())
        RETURNING id
        "#,
    )
    .bind(journey_id)
    .bind(1.0_f64)
    .bind(2.0_f64)
    .fetch_one(pool)
    .await
    .expect("failed to insert test checkpoint")
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

    let r2 = crate::journey::storage::R2::from_config(&config);

    Arc::new(crate::state::AppState {
        db,
        config,
        oauth_client,
        http_client: reqwest::Client::new(),
        pending_logins: Mutex::new(HashMap::new()),
        r2,
    })
}
