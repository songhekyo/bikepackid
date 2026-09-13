use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

const SESSION_LIFETIME_DAYS: i64 = 30;

/// Creates the DB-side record a JWT's `jti` claim points to, and returns
/// its id plus expiry so the caller can put both in the token.
pub async fn create(pool: &PgPool, user_id: Uuid) -> Result<(Uuid, chrono::DateTime<Utc>), AppError> {
    let expires_at = Utc::now() + Duration::days(SESSION_LIFETIME_DAYS);

    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO sessions (user_id, expires_at) VALUES ($1, $2) RETURNING id",
    )
    .bind(user_id)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok((id, expires_at))
}

/// True only if the session still exists, hasn't been revoked (logout,
/// ban), and hasn't expired — this is what makes a stolen-but-not-yet-
/// expired JWT actually revocable, unlike checking the JWT's own `exp`
/// claim alone.
pub async fn is_valid(pool: &PgPool, session_id: Uuid) -> Result<bool, AppError> {
    let valid: Option<bool> = sqlx::query_scalar(
        "SELECT revoked_at IS NULL AND expires_at > now() FROM sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(valid.unwrap_or(false))
}

pub async fn revoke(pool: &PgPool, session_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(session_id)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[tokio::test]
    async fn fresh_session_is_valid() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, user_id).await.unwrap();

        assert!(is_valid(&pool, session_id).await.unwrap());

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn revoked_session_is_no_longer_valid() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, user_id).await.unwrap();
        revoke(&pool, session_id).await.unwrap();

        assert!(!is_valid(&pool, session_id).await.unwrap());

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn revoking_twice_does_not_error() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, user_id).await.unwrap();
        revoke(&pool, session_id).await.unwrap();
        revoke(&pool, session_id).await.unwrap();

        assert!(!is_valid(&pool, session_id).await.unwrap());

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn unknown_session_is_not_valid() {
        let pool = test_support::pool().await;

        assert!(!is_valid(&pool, Uuid::new_v4()).await.unwrap());
    }
}
