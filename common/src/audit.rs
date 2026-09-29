use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Records a security-relevant event (login, logout, role change, content
/// takedown, ...). Best-effort: a logging failure should never break the
/// request it's describing, so callers log-and-ignore the error.
///
/// Shared across services in this monorepo that write to the same
/// `audit_logs` table (auth-service and journey-service) — not
/// business logic specific to either one, so it lives here alongside
/// error/jwt/telemetry rather than being duplicated per service.
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
