use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use oauth2::PkceCodeVerifier;
use sqlx::PgPool;

use crate::auth::google::OauthClient;
use crate::config::Config;

pub struct PendingLogin {
    pub verifier: PkceCodeVerifier,
    pub created_at: DateTime<Utc>,
}

/// Holds the PKCE verifier for a login attempt in progress, keyed by the
/// CSRF `state` value we handed to Google. A real multi-instance deployment
/// would put this in Redis (or a signed cookie) instead of process memory.
/// Entries older than `LOGIN_ATTEMPT_TTL_MINUTES` are swept out on the next
/// login attempt so an abandoned flow can't grow this map forever.
pub type PendingLogins = Mutex<HashMap<String, PendingLogin>>;

pub const LOGIN_ATTEMPT_TTL_MINUTES: i64 = 10;

pub struct AppState {
    pub db: PgPool,
    pub config: Config,
    pub oauth_client: OauthClient,
    pub http_client: reqwest::Client,
    pub pending_logins: PendingLogins,
    /// Credentials are resolved lazily per-call from the environment's
    /// default provider chain (an IAM role attached to the EC2 instance in
    /// production) — nothing here needs a static access key.
    pub ses_client: aws_sdk_sesv2::Client,
}

pub type SharedState = Arc<AppState>;
