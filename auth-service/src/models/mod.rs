// Role/User now live in the `bikepackid_common` crate (workspace root),
// shared with other services in this monorepo (e.g. journey-service) so
// they don't each redefine the same types. Re-exported here so existing
// `crate::models::{User, Role}` call sites throughout this crate don't
// need to change.
//
// `Role` is only named directly in this crate's test code (production
// handlers just read `user.role` off `User` without naming the type) —
// `allow` here rather than dropping the re-export, since it's still a
// legitimate part of this module's public surface.
#[allow(unused_imports)]
pub use bikepackid_common::user::{Role, User};
