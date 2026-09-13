use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Creator,
    Moderator,
    Admin,
    Superadmin,
}

impl Role {
    /// Only creator-and-above may authenticate into the companion app;
    /// plain viewers stay on the public web.
    pub fn can_use_app(&self) -> bool {
        !matches!(self, Role::Viewer)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub google_id: String,
    pub email: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}
