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

    /// Moderator-and-above can act on content they don't own (e.g. see a
    /// draft journey, edit someone else's journey during moderation).
    pub fn can_moderate(&self) -> bool {
        matches!(self, Role::Moderator | Role::Admin | Role::Superadmin)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_viewer_is_blocked_from_the_app() {
        assert!(!Role::Viewer.can_use_app());
        assert!(Role::Creator.can_use_app());
        assert!(Role::Moderator.can_use_app());
        assert!(Role::Admin.can_use_app());
        assert!(Role::Superadmin.can_use_app());
    }

    #[test]
    fn only_moderator_and_above_can_moderate() {
        assert!(!Role::Viewer.can_moderate());
        assert!(!Role::Creator.can_moderate());
        assert!(Role::Moderator.can_moderate());
        assert!(Role::Admin.can_moderate());
        assert!(Role::Superadmin.can_moderate());
    }
}
