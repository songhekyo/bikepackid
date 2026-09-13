use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use oauth2::{AuthorizationCode, CsrfToken, PkceCodeChallenge, Scope, TokenResponse};
use serde::Deserialize;

use crate::{
    auth::{extractor::SESSION_COOKIE, google, jwt},
    error::AppError,
    models::User,
    state::SharedState,
};

pub async fn google_login(State(state): State<SharedState>) -> impl IntoResponse {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf_token) = state
        .oauth_client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    // Google will hand this `state` value back at the callback, so we use it
    // to find the matching PKCE verifier we generated for this attempt.
    state
        .pending_logins
        .lock()
        .expect("pending_logins mutex poisoned")
        .insert(csrf_token.secret().clone(), pkce_verifier);

    Redirect::to(auth_url.as_str())
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

pub async fn google_callback(
    State(state): State<SharedState>,
    Query(query): Query<CallbackQuery>,
) -> Result<impl IntoResponse, AppError> {
    let pkce_verifier = state
        .pending_logins
        .lock()
        .expect("pending_logins mutex poisoned")
        .remove(&query.state)
        .ok_or_else(|| AppError::Oauth("unknown or expired login attempt".to_string()))?;

    let token = state
        .oauth_client
        .exchange_code(AuthorizationCode::new(query.code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))?;

    let google_user = google::fetch_user_info(token.access_token().secret()).await?;

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (google_id, email, name, avatar_url)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (google_id)
        DO UPDATE SET email = EXCLUDED.email, name = EXCLUDED.name, avatar_url = EXCLUDED.avatar_url
        RETURNING *
        "#,
    )
    .bind(&google_user.sub)
    .bind(&google_user.email)
    .bind(&google_user.name)
    .bind(&google_user.picture)
    .fetch_one(&state.db)
    .await?;

    let session_token = jwt::issue(user.id, user.role, &state.config.jwt_secret);

    let cookie = Cookie::build((SESSION_COOKIE, session_token))
        .http_only(true)
        .secure(false) // flip to true once this is served over HTTPS in production
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(30))
        .build();

    let jar = CookieJar::new().add(cookie);

    Ok((jar, Redirect::to(&state.config.frontend_url)))
}

pub async fn logout() -> impl IntoResponse {
    let expired = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build();

    (CookieJar::new().add(expired), Redirect::to("/"))
}
