// Role/User now live in the `bikepackid_common` crate (workspace root),
// shared with other services in this monorepo (e.g. journey-service) so
// they don't each redefine the same types. Re-exported here so existing
// `crate::models::{User, Role}` call sites throughout this crate don't
// need to change.
pub use bikepackid_common::user::{Role, User};
