use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{error::AppError, models::User};

/// How long a revoked/expired session row is kept around before
/// `purge_expired` deletes it — long enough to still be useful if someone
/// needs to investigate a login, short enough that the table doesn't grow
/// forever.
const EXPIRED_SESSION_RETENTION_DAYS: i64 = 7;

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

pub async fn revoke(pool: &PgPool, session_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(session_id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Revokes every active (not already revoked) session belonging to a user.
/// Used by "sign out everywhere" — unlike `revoke`, this isn't scoped to
/// one session id, so a stolen/forgotten session anywhere gets cut off in
/// one call instead of requiring the caller to know every session id.
pub async fn revoke_all(pool: &PgPool, user_id: Uuid) -> Result<u64, AppError> {
    let result = sqlx::query(
        "UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Loads the user for a session in one round trip, and structurally ties
/// the two together: `WHERE s.user_id = $2` fails closed if the session's
/// owner ever diverges from the JWT's `sub` claim, instead of relying on
/// "the two values happen to always agree" as a caller-side invariant.
pub async fn authenticate(
    pool: &PgPool,
    session_id: Uuid,
    user_id: Uuid,
) -> Result<Option<User>, AppError> {
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

/// Deletes session rows that expired more than a week ago. Meant to be
/// called on a schedule (see the background task in `main`) — sessions
/// aren't cleaned up any other way, so without this the table only grows.
pub async fn purge_expired(pool: &PgPool) -> Result<u64, AppError> {
    let result = sqlx::query(
        "DELETE FROM sessions WHERE expires_at < now() - make_interval(days => $1)",
    )
    .bind(EXPIRED_SESSION_RETENTION_DAYS as i32)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[tokio::test]
    async fn fresh_session_authenticates_its_owner() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, user_id).await.unwrap();
        let user = authenticate(&pool, session_id, user_id).await.unwrap();

        assert_eq!(user.map(|u| u.id), Some(user_id));

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn revoked_session_does_not_authenticate() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, user_id).await.unwrap();
        revoke(&pool, session_id).await.unwrap();

        assert!(authenticate(&pool, session_id, user_id)
            .await
            .unwrap()
            .is_none());

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn revoking_twice_does_not_error() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, user_id).await.unwrap();
        revoke(&pool, session_id).await.unwrap();
        revoke(&pool, session_id).await.unwrap();

        assert!(authenticate(&pool, session_id, user_id)
            .await
            .unwrap()
            .is_none());

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn revoke_all_invalidates_every_session_for_the_user() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let (session_a, _) = create(&pool, user_id).await.unwrap();
        let (session_b, _) = create(&pool, user_id).await.unwrap();

        let revoked = revoke_all(&pool, user_id).await.unwrap();
        assert_eq!(revoked, 2);

        assert!(authenticate(&pool, session_a, user_id).await.unwrap().is_none());
        assert!(authenticate(&pool, session_b, user_id).await.unwrap().is_none());

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn revoke_all_does_not_touch_another_users_sessions() {
        let pool = test_support::pool().await;
        let target_id = test_support::insert_user(&pool).await;
        let other_id = test_support::insert_user(&pool).await;

        let (target_session, _) = create(&pool, target_id).await.unwrap();
        let (other_session, _) = create(&pool, other_id).await.unwrap();

        revoke_all(&pool, target_id).await.unwrap();

        assert!(authenticate(&pool, target_session, target_id).await.unwrap().is_none());
        assert!(authenticate(&pool, other_session, other_id).await.unwrap().is_some());

        test_support::delete_user(&pool, target_id).await;
        test_support::delete_user(&pool, other_id).await;
    }

    #[tokio::test]
    async fn revoke_all_with_no_sessions_is_a_no_op() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let revoked = revoke_all(&pool, user_id).await.unwrap();
        assert_eq!(revoked, 0);

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn unknown_session_does_not_authenticate() {
        let pool = test_support::pool().await;

        assert!(authenticate(&pool, Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn session_does_not_authenticate_a_different_user() {
        let pool = test_support::pool().await;
        let owner_id = test_support::insert_user(&pool).await;
        let other_id = test_support::insert_user(&pool).await;

        let (session_id, _expires_at) = create(&pool, owner_id).await.unwrap();

        // A valid session for `owner_id` must not authenticate as anyone
        // else, even if some other bug fed it the wrong `sub` claim.
        assert!(authenticate(&pool, session_id, other_id)
            .await
            .unwrap()
            .is_none());

        test_support::delete_user(&pool, owner_id).await;
        test_support::delete_user(&pool, other_id).await;
    }
}
