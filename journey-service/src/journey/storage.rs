use std::time::Duration;

use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

use crate::config::Config;

/// Presigned upload URLs are valid for this long — long enough for a
/// mobile upload on a slow connection, short enough that a leaked URL
/// isn't useful for long.
const UPLOAD_URL_TTL: Duration = Duration::from_secs(15 * 60);

/// Wraps the R2 bucket/credentials needed to hand out presigned upload
/// URLs. This service never uploads or reads file bytes itself — see
/// docs/SYSTEM_DESIGN.md for why (keeps large transfers off this service).
pub struct R2 {
    bucket: Bucket,
    credentials: Credentials,
    public_url_base: String,
}

impl R2 {
    pub fn from_config(config: &Config) -> Self {
        let endpoint = format!("https://{}.r2.cloudflarestorage.com", config.r2_account_id)
            .parse()
            .expect("R2 endpoint must be a valid URL");

        // R2 only supports path-style addressing (bucket in the path, not
        // as a subdomain) — the account id is already the host.
        let bucket = Bucket::new(endpoint, UrlStyle::Path, config.r2_bucket_name.clone(), "auto")
            .expect("R2 endpoint must have a valid scheme and host");

        let credentials =
            Credentials::new(config.r2_access_key_id.clone(), config.r2_secret_access_key.clone());

        Self {
            bucket,
            credentials,
            public_url_base: config.r2_public_url_base.clone(),
        }
    }

    /// Generates a presigned PUT URL for a fresh, random object key, and
    /// the public (read) URL that object will have once uploaded. The
    /// caller uploads the file bytes directly to the first, then stores
    /// the second as `media_url`/`cover_image` via the normal
    /// create-post/update-journey endpoints.
    pub fn presign_upload(&self) -> (String, String) {
        let key = uuid::Uuid::new_v4().to_string();
        let action = self.bucket.put_object(Some(&self.credentials), &key);
        let upload_url = action.sign(UPLOAD_URL_TTL).to_string();
        let public_url = format!("{}/{key}", self.public_url_base.trim_end_matches('/'));

        (upload_url, public_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            database_url: String::new(),
            jwt_secret: String::new(),
            frontend_url: "http://localhost".to_string(),
            port: 8081,
            r2_account_id: "test-account".to_string(),
            r2_access_key_id: "test-key".to_string(),
            r2_secret_access_key: "test-secret".to_string(),
            r2_bucket_name: "test-bucket".to_string(),
            r2_public_url_base: "https://pub-test.r2.dev".to_string(),
        }
    }

    #[test]
    fn presign_upload_produces_a_signed_url_and_a_matching_public_url() {
        let r2 = R2::from_config(&test_config());
        let (upload_url, public_url) = r2.presign_upload();

        assert!(upload_url.starts_with("https://test-account.r2.cloudflarestorage.com/test-bucket/"));
        assert!(upload_url.contains("X-Amz-Signature"));
        assert!(public_url.starts_with("https://pub-test.r2.dev/"));

        // Same object key on both URLs — the client uploads to the first
        // and the resulting object is reachable at the second.
        let key = public_url.strip_prefix("https://pub-test.r2.dev/").unwrap();
        assert!(upload_url.contains(key));
    }
}
