use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub google_client_id: String,
    pub google_client_secret: String,
    pub google_redirect_url: String,
    pub frontend_url: String,
    pub port: u16,
    /// Whether session cookies get the `Secure` flag (HTTPS-only). Defaults
    /// to `true` — set `COOKIE_SECURE=false` only for local HTTP development.
    pub cookie_secure: bool,
    pub r2_account_id: String,
    pub r2_access_key_id: String,
    pub r2_secret_access_key: String,
    pub r2_bucket_name: String,
    /// Prefix an uploaded object's key is appended to for its public
    /// (read) URL — the r2.dev subdomain for now, a custom domain later
    /// (see docs/JOURNEY_TODO.md). Never used for the presigned *upload*
    /// URL, which always goes to R2's S3-compatible endpoint directly.
    pub r2_public_url_base: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            google_client_id: env::var("GOOGLE_CLIENT_ID").expect("GOOGLE_CLIENT_ID must be set"),
            google_client_secret: env::var("GOOGLE_CLIENT_SECRET")
                .expect("GOOGLE_CLIENT_SECRET must be set"),
            google_redirect_url: env::var("GOOGLE_REDIRECT_URL")
                .expect("GOOGLE_REDIRECT_URL must be set"),
            frontend_url: env::var("FRONTEND_URL").expect("FRONTEND_URL must be set"),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .expect("PORT must be a valid number"),
            cookie_secure: Self::parse_cookie_secure(env::var("COOKIE_SECURE").ok()),
            r2_account_id: env::var("R2_ACCOUNT_ID").expect("R2_ACCOUNT_ID must be set"),
            r2_access_key_id: env::var("R2_ACCESS_KEY_ID").expect("R2_ACCESS_KEY_ID must be set"),
            r2_secret_access_key: env::var("R2_SECRET_ACCESS_KEY")
                .expect("R2_SECRET_ACCESS_KEY must be set"),
            r2_bucket_name: env::var("R2_BUCKET_NAME").expect("R2_BUCKET_NAME must be set"),
            r2_public_url_base: env::var("R2_PUBLIC_URL_BASE")
                .expect("R2_PUBLIC_URL_BASE must be set"),
        }
    }

    /// Pulled out of `from_env` so this rule (default `true`; only an
    /// explicit `"false"` opts out) can be unit-tested without mutating
    /// real process env vars, which is process-global and flaky under
    /// parallel test execution.
    fn parse_cookie_secure(value: Option<String>) -> bool {
        value.map(|v| v != "false").unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_secure_defaults_to_true_when_unset() {
        assert!(Config::parse_cookie_secure(None));
    }

    #[test]
    fn cookie_secure_is_false_only_when_explicitly_set_to_false() {
        assert!(!Config::parse_cookie_secure(Some("false".to_string())));
        assert!(Config::parse_cookie_secure(Some("true".to_string())));
        assert!(Config::parse_cookie_secure(Some("".to_string())));
    }
}
