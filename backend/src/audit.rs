use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Records a security-relevant event (login, logout, role change, content
/// takedown, ...). Best-effort: a logging failure should never break the
/// request it's describing, so callers log-and-ignore the error.
pub async fn log(
    pool: &PgPool,
    user_id: Option<Uuid>,
    event: &str,
    metadata: Option<Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_logs (user_id, event, metadata) VALUES ($1, $2, $3)")
        .bind(user_id)
        .bind(event)
        .bind(metadata)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use serde_json::json;

    #[tokio::test]
    async fn log_records_the_event_with_its_metadata() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        log(&pool, Some(user_id), "test_event", Some(json!({ "k": "v" })))
            .await
            .unwrap();

        let metadata: Value = sqlx::query_scalar(
            "SELECT metadata FROM audit_logs WHERE user_id = $1 AND event = 'test_event'",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(metadata, json!({ "k": "v" }));

        sqlx::query("DELETE FROM audit_logs WHERE user_id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn log_accepts_no_user_and_no_metadata() {
        let pool = test_support::pool().await;

        log(&pool, None, "anonymous_event", None).await.unwrap();

        sqlx::query("DELETE FROM audit_logs WHERE event = 'anonymous_event'")
            .execute(&pool)
            .await
            .ok();
    }
}
