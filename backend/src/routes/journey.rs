use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    audit,
    auth::{AuthUser, OptionalAuthUser},
    error::AppError,
    journey::{
        self, Checkpoint, CreateCheckpointRequest, CreateJourneyRequest, CreatePostRequest,
        Journey, Post, UpdateJourneyRequest,
    },
    state::SharedState,
};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 50;

#[derive(Debug, Deserialize)]
pub struct ListJourneysQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

impl ListJourneysQuery {
    /// Clamped instead of rejecting an out-of-range value outright.
    fn clamped(&self) -> (i64, i64) {
        let limit = self.limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}

/// GET /journeys — public feed, excludes draft.
pub async fn list(
    State(state): State<SharedState>,
    Query(query): Query<ListJourneysQuery>,
) -> Result<Json<Vec<Journey>>, AppError> {
    let (limit, offset) = query.clamped();

    let journeys = journey::service::list_public_journeys(&state.db, limit, offset).await?;
    Ok(Json(journeys))
}

/// GET /me/journeys — every journey the caller owns, including `draft`
/// (the one place drafts show up in a list rather than only being
/// fetchable one at a time by id via `GET /journeys/:id`).
pub async fn list_mine(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Query(query): Query<ListJourneysQuery>,
) -> Result<Json<Vec<Journey>>, AppError> {
    let (limit, offset) = query.clamped();

    let journeys = journey::service::list_my_journeys(&state.db, user.id, limit, offset).await?;
    Ok(Json(journeys))
}

/// POST /journeys — always starts as `draft`; use PATCH to publish once
/// endpoints/etc. are filled in.
pub async fn create(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Json(req): Json<CreateJourneyRequest>,
) -> Result<(StatusCode, Json<Journey>), AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let journey = journey::service::create_journey(&state.db, user.id, req).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "journey_created",
        Some(json!({ "journey_id": journey.id })),
    )
    .await
    {
        tracing::error!(?err, "failed to write journey_created audit log");
    }

    Ok((StatusCode::CREATED, Json(journey)))
}

/// GET /journeys/:id — public unless `draft`, in which case only the
/// owner or a moderator+ (if logged in) can see it.
pub async fn get(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(journey_id): Path<Uuid>,
) -> Result<Json<Journey>, AppError> {
    let journey = journey::service::get_journey(&state.db, journey_id, viewer.as_ref()).await?;
    Ok(Json(journey))
}

/// PATCH /journeys/:id — owner or moderator+ only; every field optional
/// (unset fields are left unchanged).
pub async fn update(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(journey_id): Path<Uuid>,
    Json(req): Json<UpdateJourneyRequest>,
) -> Result<Json<Journey>, AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let journey = journey::service::update_journey(&state.db, journey_id, &user, req).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "journey_updated",
        Some(json!({ "journey_id": journey.id, "status": journey.status })),
    )
    .await
    {
        tracing::error!(?err, "failed to write journey_updated audit log");
    }

    Ok(Json(journey))
}

/// GET /journeys/:id/checkpoints — same visibility rule as the journey
/// itself; the owner/moderator sees every checkpoint, everyone else only
/// the published ones on a non-draft journey.
pub async fn list_checkpoints(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(journey_id): Path<Uuid>,
) -> Result<Json<Vec<Checkpoint>>, AppError> {
    let checkpoints =
        journey::service::list_checkpoints(&state.db, journey_id, viewer.as_ref()).await?;
    Ok(Json(checkpoints))
}

pub async fn create_checkpoint(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(journey_id): Path<Uuid>,
    Json(req): Json<CreateCheckpointRequest>,
) -> Result<(StatusCode, Json<Checkpoint>), AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let checkpoint = journey::service::create_checkpoint(&state.db, journey_id, &user, req).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "checkpoint_created",
        Some(json!({ "checkpoint_id": checkpoint.id, "journey_id": journey_id })),
    )
    .await
    {
        tracing::error!(?err, "failed to write checkpoint_created audit log");
    }

    Ok((StatusCode::CREATED, Json(checkpoint)))
}

/// GET /checkpoints/:id/posts — same visibility rule, resolved through
/// the checkpoint's parent journey.
pub async fn list_posts(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(checkpoint_id): Path<Uuid>,
) -> Result<Json<Vec<Post>>, AppError> {
    let posts = journey::service::list_posts(&state.db, checkpoint_id, viewer.as_ref()).await?;
    Ok(Json(posts))
}

pub async fn create_post(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(checkpoint_id): Path<Uuid>,
    Json(req): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<Post>), AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let post = journey::service::create_post(&state.db, checkpoint_id, &user, req).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "post_created",
        Some(json!({ "post_id": post.id, "checkpoint_id": checkpoint_id })),
    )
    .await
    {
        tracing::error!(?err, "failed to write post_created audit log");
    }

    Ok((StatusCode::CREATED, Json(post)))
}

#[derive(Debug, serde::Serialize)]
pub struct PresignUploadResponse {
    upload_url: String,
    public_url: String,
}

/// POST /uploads/presign-url — hands out a one-time presigned R2 PUT URL
/// plus the public URL the upload will be reachable at once done. Not
/// scoped to a specific journey/checkpoint (ownership only matters when
/// the resulting URL is actually saved, via update_journey/create_post) —
/// this only ever costs an R2 signature computation, no DB write, no
/// file bytes touch this server.
pub async fn presign_upload(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Json<PresignUploadResponse>, AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let (upload_url, public_url) = state.r2.presign_upload();
    Ok(Json(PresignUploadResponse { upload_url, public_url }))
}
