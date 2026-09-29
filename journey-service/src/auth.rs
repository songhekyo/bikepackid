use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::extract::CookieJar;
use bikepackid_common::{error::AppError, jwt, user::User};
use sqlx::PgPool;
use uuid::Uuid;

use crate::state::SharedState;

pub const SESSION_COOKIE: &str = "session";

/// Loads the user for a session in one round trip — deliberately duplicated
/// from `backend`'s `auth::session::authenticate` rather than shared,
/// since this is the actual service boundary: journey-service checks a
/// session's validity against the same `sessions`/`users` tables directly
/// (both services share one Postgres in this phase) instead of calling
/// back to `backend` over the network for every request. If/when the
/// services get their own databases, this is the function that would
/// change into a network call.
async fn authenticate(pool: &PgPool, session_id: Uuid, user_id: Uuid) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"
        SELECT u.id, u.google_id, u.email, u.name, u.avatar_url, u.role, u.created_at
        FROM sessions s
        JOIN users u ON u.id = s.user_id
        WHERE s.id = $1
          AND s.user_id = $2
          AND s.revoked_at IS NULL
          AND s.expires_at > now()
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

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

        let user = authenticate(&state.db, claims.jti, claims.sub)
            .await?
            .ok_or(AppError::Unauthorized)?;

        Ok(AuthUser(user))
    }
}

/// Like `AuthUser`, but never rejects the request — a missing or invalid
/// session just becomes `None`. For public read endpoints that still want
/// to show a logged-in owner their own not-yet-public content (e.g. their
/// own draft journey) without requiring login for everyone else.
pub struct OptionalAuthUser(pub Option<User>);

#[axum::async_trait]
impl FromRequestParts<SharedState> for OptionalAuthUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(AuthUser(user)) => Ok(OptionalAuthUser(Some(user))),
            Err(_) => Ok(OptionalAuthUser(None)),
        }
    }
}
