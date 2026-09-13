use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use chrono::Utc;
use oauth2::{AuthorizationCode, CsrfToken, PkceCodeChallenge, Scope, TokenResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    audit,
    auth::{extractor::SESSION_COOKIE, google, jwt, session},
    error::AppError,
    models::User,
    state::{PendingLogin, SharedState, LOGIN_ATTEMPT_TTL_MINUTES},
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

    {
        let mut pending = state
            .pending_logins
            .lock()
            .expect("pending_logins mutex poisoned");

        // Sweep abandoned login attempts so this map can't grow forever if
        // people start the Google redirect and never come back.
        let cutoff = Utc::now() - chrono::Duration::minutes(LOGIN_ATTEMPT_TTL_MINUTES);
        pending.retain(|_, entry| entry.created_at > cutoff);

        // Google will hand this `state` value back at the callback, so we
        // use it to find the matching PKCE verifier for this attempt.
        pending.insert(
            csrf_token.secret().clone(),
            PendingLogin {
                verifier: pkce_verifier,
                created_at: Utc::now(),
            },
        );
    }

    Redirect::to(auth_url.as_str())
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    /// Absent when the user declined consent — see `error` below.
    code: Option<String>,
    state: String,
    /// Set by Google (e.g. `access_denied`) when the user cancels the
    /// consent screen instead of completing it. Not an error on our end.
    error: Option<String>,
}

pub async fn google_callback(
    State(state): State<SharedState>,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    let pending = {
        let mut logins = state
            .pending_logins
            .lock()
            .expect("pending_logins mutex poisoned");
        logins.remove(&query.state)
    };

    if let Some(error) = query.error {
        tracing::info!(%error, "user declined the Google consent screen");
        return Ok(Redirect::to(&format!("{}?login=cancelled", state.config.frontend_url)).into_response());
    }

    let cutoff = Utc::now() - chrono::Duration::minutes(LOGIN_ATTEMPT_TTL_MINUTES);
    let pkce_verifier = match pending {
        Some(entry) if entry.created_at > cutoff => entry.verifier,
        _ => {
            return Err(AppError::BadRequest(
                "unknown or expired login attempt".to_string(),
            ))
        }
    };

    let code = query
        .code
        .ok_or_else(|| AppError::BadRequest("missing code from Google callback".to_string()))?;

    let token = state
        .oauth_client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&state.http_client)
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))?;

    let google_user = google::fetch_user_info(&state.http_client, token.access_token().secret()).await?;

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

    let (session_id, expires_at) = session::create(&state.db, user.id).await?;
    let session_token = jwt::issue(user.id, user.role, session_id, expires_at, &state.config.jwt_secret);

    if let Err(err) = audit::log(&state.db, Some(user.id), "login", Some(json!({ "method": "google" }))).await
    {
        tracing::error!(?err, "failed to write login audit log");
    }

    // Derived from the session's real expiry (not a separately-hardcoded
    // duration) so the cookie and the DB-side session can never disagree
    // about how long the login lasts.
    let max_age_seconds = (expires_at - Utc::now()).num_seconds().max(0);

    let cookie = Cookie::build((SESSION_COOKIE, session_token))
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(max_age_seconds))
        .build();

    let jar = CookieJar::new().add(cookie);

    Ok((jar, Redirect::to(&state.config.frontend_url)).into_response())
}

pub async fn logout(State(state): State<SharedState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        if let Some(claims) = jwt::verify(cookie.value(), &state.config.jwt_secret) {
            if let Err(err) = session::revoke(&state.db, claims.jti).await {
                tracing::error!(?err, "failed to revoke session on logout");
            }
            if let Err(err) = audit::log(&state.db, Some(claims.sub), "logout", None).await {
                tracing::error!(?err, "failed to write logout audit log");
            }
        }
    }

    let expired = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build();

    (jar.add(expired), Redirect::to("/"))
}
