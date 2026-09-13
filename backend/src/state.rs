use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use oauth2::PkceCodeVerifier;
use sqlx::PgPool;

use crate::auth::google::OauthClient;
use crate::config::Config;

/// Holds the PKCE verifier for a login attempt in progress, keyed by the
/// CSRF `state` value we handed to Google. A real multi-instance deployment
/// would put this in Redis (or a signed cookie) instead of process memory.
pub type PendingLogins = Mutex<HashMap<String, PkceCodeVerifier>>;

pub struct AppState {
    pub db: PgPool,
    pub config: Config,
    pub oauth_client: OauthClient,
    pub pending_logins: PendingLogins,
}

pub type SharedState = Arc<AppState>;
