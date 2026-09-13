pub mod auth;
pub mod me;

use axum::{
    routing::{get, post},
    Router,
};

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/auth/google/login", get(auth::google_login))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(me::me))
        .route("/app/status", get(me::app_status))
}
