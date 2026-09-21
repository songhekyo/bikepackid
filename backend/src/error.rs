use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("database error: {0}")]
    Database(sqlx::Error),

    #[error("google oauth error: {0}")]
    Oauth(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("not found")]
    NotFound,
}

// A CHECK constraint violation (e.g. journeys_endpoints_required_outside_draft)
// means the client sent data that's invalid by the DB's own rules — that's
// a 400, not a server bug, even though it arrives via the same sqlx::Error
// as a real failure. Done here (at conversion time, via `?`) rather than in
// `into_response` below, so callers that match on `AppError` directly
// (tests, future service-layer logic) see `BadRequest` too, not just HTTP
// responses.
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = err {
            if db_err.code().as_deref() == Some("23514") {
                return AppError::BadRequest(db_err.message().to_string());
            }
        }

        AppError::Database(err)
    }
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
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
