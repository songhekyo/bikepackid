pub mod extractor;
pub mod google;
pub mod session;

// Moved to `bikepackid_common` — journey-service verifies the same JWT
// format/secret and shouldn't redefine `Claims`/issue/verify separately.
// Re-exported here so existing `crate::auth::jwt::X` call sites in this
// crate don't need to change.
pub use bikepackid_common::jwt;

pub use extractor::AuthUser;
