use axum::Json;

use crate::{auth::AuthUser, error::AppError, models::User};

/// GET /me — works for any logged-in user (web viewer or app creator/admin).
pub async fn me(AuthUser(user): AuthUser) -> Json<User> {
    Json(user)
}

/// GET /app/status — a stand-in for every app-only route: proves the
/// role gate works before any real app endpoints (journeys, moderation,
/// marketplace admin) get built on top of it.
pub async fn app_status(AuthUser(user): AuthUser) -> Result<Json<User>, AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }
    Ok(Json(user))
}
