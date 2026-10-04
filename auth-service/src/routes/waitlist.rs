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
    if email.is_empty() {
        return Err(AppError::BadRequest("email must not be empty".to_string()));
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
