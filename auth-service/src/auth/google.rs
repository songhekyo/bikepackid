use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl, TokenUrl,
};
use serde::Deserialize;
use sqlx::PgPool;

use crate::{error::AppError, models::User};

/// oauth2 v5 encodes "which endpoints are configured" in the type itself
/// (`EndpointSet`/`EndpointNotSet`), so the client's type has to spell out
/// exactly which ones `build_client` sets: auth + token uri, not the
/// optional device-auth/introspection/revocation ones.
pub type OauthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

pub fn build_client(client_id: &str, client_secret: &str, redirect_url: &str) -> OauthClient {
    BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.to_string()))
        .set_auth_uri(
            AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
                .expect("hardcoded Google auth URL is valid"),
        )
        .set_token_uri(
            TokenUrl::new("https://oauth2.googleapis.com/token".to_string())
                .expect("hardcoded Google token URL is valid"),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_url.to_string())
                .expect("GOOGLE_REDIRECT_URL must be a valid URL"),
        )
}

#[derive(Debug, Deserialize)]
pub struct GoogleUserInfo {
    pub sub: String,
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
}

/// Calls Google's userinfo endpoint with the access token we just received,
/// to find out who actually logged in.
pub async fn fetch_user_info(
    http_client: &reqwest::Client,
    access_token: &str,
) -> Result<GoogleUserInfo, AppError> {
    let response = http_client
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))?;

    if !response.status().is_success() {
        return Err(AppError::Oauth(format!(
            "userinfo request failed with status {}",
            response.status()
        )));
    }

    response
        .json::<GoogleUserInfo>()
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct GoogleIdTokenInfo {
    pub aud: String,
    pub sub: String,
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
}

/// Verifies a Google ID token (the credential the mobile app holds
/// on-device after its own Google sign-in, as opposed to the web flow's
/// authorization code + access token) via Google's `tokeninfo` endpoint —
/// signature, expiry, and audience are all checked by Google itself, so
/// this needs no local JWKS fetching/caching/rotation logic. Tradeoff: one
/// extra network round trip per mobile login (still cheaper overall than
/// the web flow's two Google calls) and `tokeninfo` is documented more as
/// a debugging aid than a guaranteed-SLA endpoint — worth revisiting with
/// local JWKS verification behind this same signature if mobile login
/// volume ever justifies it.
pub async fn verify_id_token(
    http_client: &reqwest::Client,
    id_token: &str,
    expected_aud: &str,
) -> Result<GoogleIdTokenInfo, AppError> {
    let response = http_client
        .get("https://oauth2.googleapis.com/tokeninfo")
        .query(&[("id_token", id_token)])
        .send()
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))?;

    // A non-2xx here means Google is saying the token itself is
    // bad/expired/malformed — that's the caller's credential, not a
    // problem with our plumbing to Google.
    if !response.status().is_success() {
        return Err(AppError::Unauthorized);
    }

    let info: GoogleIdTokenInfo = response
        .json()
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))?;

    if info.aud != expected_aud {
        return Err(AppError::Unauthorized);
    }

    Ok(info)
}

/// Upserts a user by Google id — the exact query `google_callback` used
/// inline before this had a second call site (the mobile login endpoint),
/// factored out so the two can't silently drift apart.
pub async fn upsert_user(
    pool: &PgPool,
    google_id: &str,
    email: &str,
    name: &str,
    picture: Option<&str>,
) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (google_id, email, name, avatar_url)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (google_id)
        DO UPDATE SET email = EXCLUDED.email, name = EXCLUDED.name, avatar_url = EXCLUDED.avatar_url
        RETURNING *
        "#,
    )
    .bind(google_id)
    .bind(email)
    .bind(name)
    .bind(picture)
    .fetch_one(pool)
    .await?;

    Ok(user)
}

#[cfg(test)]
mod upsert_tests {
    use super::*;
    use crate::test_support;

    #[tokio::test]
    async fn fresh_google_id_inserts_a_new_user() {
        let pool = test_support::pool().await;
        let google_id = format!("test-google-id-{}", uuid::Uuid::new_v4());

        let user = upsert_user(&pool, &google_id, "rider@example.com", "Rider", None)
            .await
            .unwrap();

        assert_eq!(user.google_id, google_id);
        assert_eq!(user.email, "rider@example.com");
        assert_eq!(user.name, "Rider");

        test_support::delete_user(&pool, user.id).await;
    }

    #[tokio::test]
    async fn repeat_google_id_updates_the_same_row_instead_of_duplicating() {
        let pool = test_support::pool().await;
        let google_id = format!("test-google-id-{}", uuid::Uuid::new_v4());

        let first = upsert_user(&pool, &google_id, "old@example.com", "Old Name", None)
            .await
            .unwrap();
        let second = upsert_user(
            &pool,
            &google_id,
            "new@example.com",
            "New Name",
            Some("https://example.com/avatar.png"),
        )
        .await
        .unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(second.email, "new@example.com");
        assert_eq!(second.name, "New Name");
        assert_eq!(
            second.avatar_url,
            Some("https://example.com/avatar.png".to_string())
        );

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE google_id = $1")
            .bind(&google_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);

        test_support::delete_user(&pool, second.id).await;
    }
}
