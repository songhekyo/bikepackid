use chrono::{DateTime, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::Role;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub jti: Uuid,
    pub role: Role,
    pub exp: i64,
}

/// `jti` and `expires_at` come from a `sessions` row (see `auth::session`),
/// so the JWT's lifetime always matches a record we can revoke server-side.
pub fn issue(user_id: Uuid, role: Role, jti: Uuid, expires_at: DateTime<Utc>, secret: &str) -> String {
    let claims = Claims {
        sub: user_id,
        jti,
        role,
        exp: expires_at.timestamp(),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("JWT encoding should not fail for well-formed claims")
}

pub fn verify(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn valid_token_round_trips() {
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let expires_at = Utc::now() + Duration::days(1);

        let token = issue(user_id, Role::Creator, session_id, expires_at, "test-secret");
        let claims = verify(&token, "test-secret").expect("token should verify");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.jti, session_id);
        assert_eq!(claims.role, Role::Creator);
    }

    #[test]
    fn token_signed_with_a_different_secret_is_rejected() {
        let token = issue(
            Uuid::new_v4(),
            Role::Viewer,
            Uuid::new_v4(),
            Utc::now() + Duration::days(1),
            "secret-a",
        );

        assert!(verify(&token, "secret-b").is_none());
    }

    #[test]
    fn expired_token_is_rejected() {
        let token = issue(
            Uuid::new_v4(),
            Role::Viewer,
            Uuid::new_v4(),
            Utc::now() - Duration::days(1),
            "test-secret",
        );

        assert!(verify(&token, "test-secret").is_none());
    }
}
