use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("google oauth error: {0}")]
    Oauth(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,
}

// Axum calls this to turn our error into an actual HTTP response whenever a
// handler returns `Err(AppError)` — it's the same mechanism `Ok(_)` uses.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Database(err) => {
                tracing::error!(?err, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".to_string())
            }
            AppError::Oauth(msg) => {
                tracing::error!(%msg, "oauth error");
                (StatusCode::BAD_GATEWAY, "google login failed".to_string())
            }
            AppError::BadRequest(msg) => {
                tracing::info!(%msg, "bad request");
                (StatusCode::BAD_REQUEST, msg.clone())
            }
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".to_string()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
