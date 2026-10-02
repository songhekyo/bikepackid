pub mod service;
pub mod storage;

use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// TTL-only cache for `GET /journeys` (the public feed) — see the comment
/// on the `moka` dependency in Cargo.toml for why in-process + TTL-only,
/// not Redis or invalidate-on-write. Keyed by `(limit, offset)`, the full
/// set of inputs that determines the result of `list_public_journeys`.
pub type JourneyListCache = moka::future::Cache<(i64, i64), Vec<Journey>>;

const JOURNEY_LIST_CACHE_TTL_SECONDS: u64 = 30;
const JOURNEY_LIST_CACHE_MAX_CAPACITY: u64 = 100;

pub fn new_journey_list_cache() -> JourneyListCache {
    moka::future::Cache::builder()
        .max_capacity(JOURNEY_LIST_CACHE_MAX_CAPACITY)
        .time_to_live(Duration::from_secs(JOURNEY_LIST_CACHE_TTL_SECONDS))
        .build()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "journey_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum JourneyStatus {
    Draft,
    Planning,
    Published,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "checkpoint_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum CheckpointStatus {
    Published,
    Flagged,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "post_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PostType {
    Photo,
    Video,
    Text,
    ThreadItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "post_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PostStatus {
    Published,
    Flagged,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "equipment_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum EquipmentStatus {
    Published,
    Flagged,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "sponsor_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SponsorStatus {
    Published,
    Flagged,
    Removed,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Journey {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: JourneyStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub cover_image: Option<String>,
    pub start_lat: Option<f64>,
    pub start_lng: Option<f64>,
    pub end_lat: Option<f64>,
    pub end_lng: Option<f64>,
    pub seeking_sponsor: bool,
    pub donation_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Checkpoint {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub lat: f64,
    pub lng: f64,
    pub captured_at: DateTime<Utc>,
    pub title: Option<String>,
    pub trigger_type: String,
    pub status: CheckpointStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Post {
    pub id: Uuid,
    pub checkpoint_id: Uuid,
    pub r#type: PostType,
    pub body: Option<String>,
    pub media_url: Option<String>,
    pub parent_post_id: Option<Uuid>,
    pub status: PostStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EquipmentCategory {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Equipment {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub category_id: Uuid,
    pub name: String,
    pub brand: Option<String>,
    pub product_url: Option<String>,
    pub notes: Option<String>,
    pub status: EquipmentStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Sponsor {
    pub id: Uuid,
    pub journey_id: Uuid,
    pub name: String,
    pub logo_url: Option<String>,
    pub website_url: Option<String>,
    pub notes: Option<String>,
    pub status: SponsorStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateJourneyRequest {
    pub title: String,
    pub description: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub cover_image: Option<String>,
}

/// All fields optional (PATCH semantics) — a `None` field is left
/// unchanged, not cleared. There's currently no way to explicitly clear a
/// nullable field back to `NULL` once set; acceptable for this pass, not
/// worth the extra complexity (an `Option<Option<T>>`-style wrapper) until
/// there's a real need for it.
#[derive(Debug, Deserialize, Default)]
pub struct UpdateJourneyRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<JourneyStatus>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub cover_image: Option<String>,
    pub start_lat: Option<f64>,
    pub start_lng: Option<f64>,
    pub end_lat: Option<f64>,
    pub end_lng: Option<f64>,
    pub seeking_sponsor: Option<bool>,
    pub donation_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCheckpointRequest {
    /// Client-suppliable so a future offline-capture flow can generate the
    /// id at capture time — a retried sync after a dropped connection is
    /// then idempotent instead of creating a duplicate. Falls back to a
    /// server-generated id when absent (every caller today).
    pub id: Option<Uuid>,
    pub lat: f64,
    pub lng: f64,
    pub captured_at: DateTime<Utc>,
    pub title: Option<String>,
    pub trigger_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePostRequest {
    pub r#type: PostType,
    pub body: Option<String>,
    pub media_url: Option<String>,
    pub parent_post_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEquipmentRequest {
    pub category_id: Uuid,
    pub name: String,
    pub brand: Option<String>,
    pub product_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSponsorRequest {
    pub name: String,
    pub logo_url: Option<String>,
    pub website_url: Option<String>,
    pub notes: Option<String>,
}
