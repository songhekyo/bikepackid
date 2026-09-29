use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

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

/// The commit that produced the running binary, baked in at Docker build
/// time (`ci.yml` passes `--build-arg GIT_SHA=$GITHUB_SHA`) — not a runtime
/// env var, so there's nothing to misconfigure on the VPS. Falls back to
/// "dev" for a plain local `cargo build`, which never sets it.
const GIT_SHA: &str = match option_env!("GIT_SHA") {
    Some(sha) => sha,
    None => "dev",
};

/// GET /version — lets you confirm which commit is actually live (e.g.
/// `curl https://domain/version`) without SSHing into the VPS to inspect
/// Docker image digests by hand.
pub async fn version() -> Json<serde_json::Value> {
    Json(json!({ "git_sha": GIT_SHA }))
}
