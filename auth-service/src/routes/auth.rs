use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use chrono::Utc;
use oauth2::{AuthorizationCode, CsrfToken, PkceCodeChallenge, Scope, TokenResponse};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use serde_json::json;
use tracing::Instrument;

/// Same "unreserved" set JavaScript's `encodeURIComponent` leaves alone.
/// `NON_ALPHANUMERIC` on its own also escapes `-_.~`, which is safe but
/// turns a plain OAuth error code like `access_denied` into the needlessly
/// noisy `access%5Fdenied`.
const QUERY_VALUE_SAFE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

use crate::{
    audit,
    auth::{extractor::SESSION_COOKIE, google, jwt, session, AuthUser},
    config::Config,
    email,
    error::AppError,
    models::User,
    state::{PendingLogin, SharedState, LOGIN_ATTEMPT_TTL_MINUTES},
};

/// Applies `Config::cookie_domain` (unset in local dev, `taktikdansiasat.com`
/// in production) to a session cookie — shared by every place this crate
/// builds or clears the `session` cookie, so the three call sites (login,
/// logout, sign-out-everywhere) can't drift out of sync on which one
/// remembers to scope it. Clearing a cookie set with a `Domain` attribute
/// requires the clearing cookie to carry the same `Domain`, or the browser
/// treats it as an unrelated cookie and the original one never actually
/// gets removed — so this matters for logout too, not just login.
fn with_shared_domain(mut cookie: Cookie<'static>, config: &Config) -> Cookie<'static> {
    if let Some(domain) = &config.cookie_domain {
        cookie.set_domain(domain.clone());
    }
    cookie
}

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
    /// Absent when Google reported an error instead of completing the
    /// flow — see `error` below.
    code: Option<String>,
    state: String,
    /// Set by Google (e.g. `access_denied` when the user cancels the
    /// consent screen) instead of `code` when the flow didn't complete.
    /// We don't interpret this — it's forwarded to the frontend as-is so
    /// the frontend owns deciding what each value means and how to
    /// present it.
    error: Option<String>,
}

pub async fn google_callback(
    State(state): State<SharedState>,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    // Validate the login attempt itself *before* looking at `error` — an
    // unrecognized or expired `state` is a bad request regardless of what
    // `error` claims, otherwise anyone could hit this endpoint with an
    // arbitrary `state` + `error` and get it reflected back at them.
    // Wrapped in its own span so the trace waterfall shows this validation
    // step separately from the network calls below it — useful for exactly
    // the kind of "which step failed" question a flat single-span trace
    // can't answer.
    let pending_entry = tracing::info_span!("validate_pending_login").in_scope(|| {
        let pending = {
            let mut logins = state
                .pending_logins
                .lock()
                .expect("pending_logins mutex poisoned");
            logins.remove(&query.state)
        };

        let cutoff = Utc::now() - chrono::Duration::minutes(LOGIN_ATTEMPT_TTL_MINUTES);
        match pending {
            Some(entry) if entry.created_at > cutoff => Ok(entry),
            _ => Err(AppError::BadRequest(
                "unknown or expired login attempt".to_string(),
            )),
        }
    })?;

    if let Some(error) = query.error {
        tracing::info!(%error, "google returned an error instead of completing the login");
        let encoded_error = percent_encoding::utf8_percent_encode(&error, QUERY_VALUE_SAFE);
        let redirect_url = format!("{}?error={encoded_error}", state.config.frontend_url);
        return Ok(Redirect::to(&redirect_url).into_response());
    }

    let pkce_verifier = pending_entry.verifier;

    let code = query
        .code
        .ok_or_else(|| AppError::BadRequest("missing code from Google callback".to_string()))?;

    let token = state
        .oauth_client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&state.http_client)
        .instrument(tracing::info_span!("exchange_code_with_google"))
        .await
        .map_err(|e| AppError::Oauth(e.to_string()))?;

    let google_user = google::fetch_user_info(&state.http_client, token.access_token().secret())
        .instrument(tracing::info_span!("fetch_google_profile"))
        .await?;

    // Checked before the upsert below so a brand-new signup can be told
    // apart from a returning user's login — the welcome email must only go
    // out once. Two first-ever logins for the same `google_id` racing each
    // other could both see "doesn't exist yet" and both send the email;
    // the upsert itself stays correct either way (`ON CONFLICT` still only
    // ever produces one row), so the only cost of that rare race is an
    // occasional duplicate email, not a data problem.
    let is_new_user = !sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM users WHERE google_id = $1)",
    )
    .bind(&google_user.sub)
    .fetch_one(&state.db)
    .instrument(tracing::info_span!("check_existing_user"))
    .await?;

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
    .instrument(tracing::info_span!("upsert_user"))
    .await?;

    let (session_id, expires_at) = session::create(&state.db, user.id).await?;
    let session_token = jwt::issue(user.id, user.role, session_id, expires_at, &state.config.jwt_secret);

    if let Err(err) = audit::log(&state.db, Some(user.id), "login", Some(json!({ "method": "google" }))).await
    {
        tracing::error!(?err, "failed to write login audit log");
    }

    // Only ever fires once per user, right after their very first login —
    // and only once the Expo project this links to actually exists
    // (`APP_INSTALL_URL` stays unset until then, see `Config::app_install_url`).
    if is_new_user {
        if let Some(install_url) = &state.config.app_install_url {
            if let Err(err) =
                email::send_welcome_email(&state.ses_client, install_url, &user.email, &user.name)
                    .instrument(tracing::info_span!("send_welcome_email"))
                    .await
            {
                tracing::error!(?err, "failed to send welcome email");
            }
        }
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
    let cookie = with_shared_domain(cookie, &state.config);

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
    let expired = with_shared_domain(expired, &state.config);

    (jar.add(expired), Redirect::to("/"))
}

/// POST /auth/sign-out-everywhere — revokes every session belonging to the
/// caller, not just the one tied to their current cookie. For when a user
/// suspects a session was stolen (malware, a device they forgot logged in
/// somewhere) and wants to cut off access everywhere at once, rather than
/// having to know which specific session to revoke.
pub async fn sign_out_everywhere(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let revoked = session::revoke_all(&state.db, user.id).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "sign_out_everywhere",
        Some(json!({ "sessions_revoked": revoked })),
    )
    .await
    {
        tracing::error!(?err, "failed to write sign_out_everywhere audit log");
    }

    // The caller's own current session is included in "everywhere", so
    // clear their cookie too — otherwise their browser keeps sending a
    // token that's now revoked (correctly unauthorized, but a needless 401
    // on their very next request instead of a clean logged-out state).
    let expired = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build();
    let expired = with_shared_domain(expired, &state.config);

    Ok((CookieJar::new().add(expired), StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod domain_tests {
    use super::*;

    fn config_with_domain(domain: Option<&str>) -> Config {
        Config {
            database_url: String::new(),
            jwt_secret: String::new(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
            google_redirect_url: String::new(),
            frontend_url: String::new(),
            port: 8080,
            cookie_secure: true,
            app_install_url: None,
            cookie_domain: domain.map(str::to_string),
        }
    }

    #[test]
    fn leaves_cookie_domain_unset_when_config_has_none() {
        let cookie = Cookie::build((SESSION_COOKIE, "token")).build();
        let cookie = with_shared_domain(cookie, &config_with_domain(None));
        assert!(cookie.domain().is_none());
    }

    #[test]
    fn sets_cookie_domain_when_config_has_one() {
        let cookie = Cookie::build((SESSION_COOKIE, "token")).build();
        let cookie = with_shared_domain(cookie, &config_with_domain(Some("taktikdansiasat.com")));
        assert_eq!(cookie.domain(), Some("taktikdansiasat.com"));
    }

    // A leading dot is the old RFC 2965 convention — harmless to pass, but
    // the `cookie` crate strips it since RFC 6265 made it redundant. This
    // pins that normalization so `COOKIE_DOMAIN` works the same whichever
    // form someone sets it to in `.env`.
    #[test]
    fn strips_a_redundant_leading_dot() {
        let cookie = Cookie::build((SESSION_COOKIE, "token")).build();
        let cookie = with_shared_domain(cookie, &config_with_domain(Some(".taktikdansiasat.com")));
        assert_eq!(cookie.domain(), Some("taktikdansiasat.com"));
    }
}
