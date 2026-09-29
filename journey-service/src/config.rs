use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    /// Must match `auth-service`'s JWT_SECRET exactly — this service verifies
    /// tokens `auth-service` issued at login, it never issues its own.
    pub jwt_secret: String,
    pub frontend_url: String,
    pub port: u16,
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
            frontend_url: env::var("FRONTEND_URL").expect("FRONTEND_URL must be set"),
            port: env::var("PORT")
                .unwrap_or_else(|_| "8081".to_string())
                .parse()
                .expect("PORT must be a valid number"),
            r2_account_id: env::var("R2_ACCOUNT_ID").expect("R2_ACCOUNT_ID must be set"),
            r2_access_key_id: env::var("R2_ACCESS_KEY_ID").expect("R2_ACCESS_KEY_ID must be set"),
            r2_secret_access_key: env::var("R2_SECRET_ACCESS_KEY")
                .expect("R2_SECRET_ACCESS_KEY must be set"),
            r2_bucket_name: env::var("R2_BUCKET_NAME").expect("R2_BUCKET_NAME must be set"),
            r2_public_url_base: env::var("R2_PUBLIC_URL_BASE")
                .expect("R2_PUBLIC_URL_BASE must be set"),
        }
    }
}
