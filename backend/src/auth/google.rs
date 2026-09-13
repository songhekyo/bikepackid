use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
use serde::Deserialize;

use crate::error::AppError;

pub type OauthClient = BasicClient;

pub fn build_client(client_id: &str, client_secret: &str, redirect_url: &str) -> OauthClient {
    BasicClient::new(
        ClientId::new(client_id.to_string()),
        Some(ClientSecret::new(client_secret.to_string())),
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("hardcoded Google auth URL is valid"),
        Some(
            TokenUrl::new("https://oauth2.googleapis.com/token".to_string())
                .expect("hardcoded Google token URL is valid"),
        ),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_url.to_string()).expect("GOOGLE_REDIRECT_URL must be a valid URL"))
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
pub async fn fetch_user_info(access_token: &str) -> Result<GoogleUserInfo, AppError> {
    let response = reqwest::Client::new()
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
