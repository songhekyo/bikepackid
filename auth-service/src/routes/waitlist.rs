use axum::{
    extract::{Form, State},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use crate::{error::AppError, state::SharedState};

#[derive(Deserialize)]
pub struct WaitlistSignup {
    email: String,
}

/// POST /api/waitlist — the landing page's only call-to-action
/// (`web/index.html`'s `<form action="/api/waitlist" method="post">`), so
/// it's a plain HTML form submit (`application/x-www-form-urlencoded`), not
/// JSON — hence `Form<_>` rather than `Json<_>`, and a redirect response
/// rather than a status code, so a non-JS browser lands back on the page
/// instead of a bare 303 with nothing to look at.
///
/// Just records the email; onboarding someone from here into `users` with
/// a real role is a manual step once there's capacity, not automatic.
pub async fn create(
    State(state): State<SharedState>,
    Form(body): Form<WaitlistSignup>,
) -> Result<Response, AppError> {
    let email = body.email.trim();
    if !is_plausible_email(email) {
        return Err(AppError::BadRequest("email is not valid".to_string()));
    }

    // A repeat submit (double-click, browser back-button resubmit) is a
    // no-op, not an error — nothing about the landing page's form lets a
    // visitor tell whether their email is already on the list.
    sqlx::query("INSERT INTO waitlist_signups (email) VALUES ($1) ON CONFLICT (email) DO NOTHING")
        .bind(email)
        .execute(&state.db)
        .await?;

    Ok(Redirect::to(&state.config.frontend_url).into_response())
}

/// RFC 5321's own mailbox length ceiling — a cheap, honest bound rather
/// than an attempt at full address validation (notoriously impossible to
/// get exactly right with a regex). Only guards against obvious junk;
/// anything that passes this but isn't real just bounces silently whenever
/// someone eventually emails it.
const MAX_EMAIL_LEN: usize = 254;

fn is_plausible_email(email: &str) -> bool {
    email.len() <= MAX_EMAIL_LEN
        && !email.is_empty()
        && email.contains('@')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_ordinary_address() {
        assert!(is_plausible_email("rider@example.com"));
    }

    #[test]
    fn rejects_empty_missing_at_whitespace_or_control_chars() {
        assert!(!is_plausible_email(""));
        assert!(!is_plausible_email("not-an-email"));
        assert!(!is_plausible_email("rider @example.com"));
        assert!(!is_plausible_email("rider@example.com\n"));
        assert!(!is_plausible_email("rider@exam\u{0}ple.com"));
    }

    #[test]
    fn rejects_an_address_longer_than_rfc_5321_allows() {
        let too_long = format!("{}@example.com", "a".repeat(MAX_EMAIL_LEN));
        assert!(!is_plausible_email(&too_long));
    }
}
