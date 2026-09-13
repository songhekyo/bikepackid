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
