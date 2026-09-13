use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::extract::CookieJar;

use crate::{error::AppError, models::User, state::SharedState};

use super::jwt;

pub const SESSION_COOKIE: &str = "session";

/// Drop `AuthUser` in any handler's argument list to require a logged-in
/// user — axum runs this before the handler body, so a missing/invalid
/// session never reaches your route logic at all.
pub struct AuthUser(pub User);

#[axum::async_trait]
impl FromRequestParts<SharedState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_string())
            .ok_or(AppError::Unauthorized)?;

        let claims = jwt::verify(&token, &state.config.jwt_secret).ok_or(AppError::Unauthorized)?;

        let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(claims.sub)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::Unauthorized)?;

        Ok(AuthUser(user))
    }
}
