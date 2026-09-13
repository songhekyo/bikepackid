use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::state::SharedState;

/// Readiness check for load balancers / orchestrators (Railway, Render,
/// Fly, k8s, ...): actually touches the database instead of returning a
/// static 200, so a DB outage shows up here instead of every other route
/// failing silently on the platform's health dashboard.
pub async fn health(State(state): State<SharedState>) -> Response {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => {
            tracing::error!(?err, "health check failed: database unreachable");
            (StatusCode::SERVICE_UNAVAILABLE, "database unreachable").into_response()
        }
    }
}
