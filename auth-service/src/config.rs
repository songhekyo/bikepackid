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
    /// The `exp://...` (or published `https://u.expo.dev/...`) link testers
    /// open in Expo Go to install the mobile app. Left unset until that
    /// project actually exists — `email::send_welcome_email` is skipped
    /// entirely while this is `None`, so merging the SES integration ahead
    /// of the app itself can't ever send a broken link.
    pub app_install_url: Option<String>,
    /// Scopes the session cookie to `taktikdansiasat.com` so the shop
    /// (`shop.taktikdansiasat.com`, a separate repo/service) can read the
    /// same login — per RFC 6265, a `Domain` attribute without a leading
    /// dot already covers subdomains too (the old RFC 2965 leading-dot
    /// convention is unnecessary with modern browsers; the `cookie` crate
    /// strips one if you pass it anyway). Left unset by default: a cookie
    /// scoped to `taktikdansiasat.com` is never sent to `localhost`, so
    /// this must stay `None` for local dev and only be set in production.
    pub cookie_domain: Option<String>,
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
            app_install_url: env::var("APP_INSTALL_URL").ok(),
            cookie_domain: env::var("COOKIE_DOMAIN").ok(),
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
