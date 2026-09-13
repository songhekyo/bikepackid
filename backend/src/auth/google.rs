use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, ClientId, ClientSecret, EndpointNotSet, EndpointSet, RedirectUrl, TokenUrl,
};
use serde::Deserialize;

use crate::error::AppError;

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
