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
