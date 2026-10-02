use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bikepackid_common::{audit, error::AppError};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    auth::{AuthUser, OptionalAuthUser},
    journey::{
        self, Checkpoint, CreateCheckpointRequest, CreateEquipmentRequest, CreateJourneyRequest,
        CreatePostRequest, CreateSponsorRequest, Equipment, EquipmentCategory, Journey, Post,
        Sponsor, UpdateJourneyRequest,
    },
    state::SharedState,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/health", get(health))
        .route("/version", get(version))
        .route("/me/journeys", get(list_mine))
        .route("/journeys", get(list).post(create))
        .route("/journeys/:id", get(get_journey).patch(update))
        .route(
            "/journeys/:id/checkpoints",
            get(list_checkpoints).post(create_checkpoint),
        )
        .route("/checkpoints/:id/posts", get(list_posts).post(create_post))
        .route(
            "/journeys/:id/equipment",
            get(list_equipment).post(create_equipment),
        )
        .route("/equipment-categories", get(list_equipment_categories))
        .route(
            "/journeys/:id/sponsors",
            get(list_sponsors).post(create_sponsor),
        )
        .route("/uploads/presign-url", post(presign_upload))
}

/// Readiness check for load balancers/orchestrators — actually touches the
/// database instead of returning a static 200. Mirrors `auth-service`'s
/// `routes::health::health` (see docs/INFRA_HISTORY.md for why it's
/// wired this way).
async fn health(State(state): State<SharedState>) -> Response {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => {
            tracing::error!(?err, "health check failed: database unreachable");
            (StatusCode::SERVICE_UNAVAILABLE, "database unreachable").into_response()
        }
    }
}

/// The commit that produced the running binary, baked in at Docker build
/// time. Falls back to "dev" for a plain local `cargo build`.
const GIT_SHA: &str = match option_env!("GIT_SHA") {
    Some(sha) => sha,
    None => "dev",
};

async fn version() -> Json<serde_json::Value> {
    Json(json!({ "git_sha": GIT_SHA }))
}

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

/// GET /journeys — public feed, excludes draft. Cached in-process for
/// `JOURNEY_LIST_CACHE_TTL_SECONDS` (see `journey::new_journey_list_cache`)
/// — a plain get-then-insert, not moka's `try_get_with`, so two requests
/// racing on the same empty cache entry both hit the DB once each rather
/// than one waiting on the other; fine at this traffic level, and simpler
/// to read than threading a `sqlx::Error` back out of an `Arc` (what
/// `try_get_with`'s dedup would require, since `AppError` isn't `Clone`).
async fn list(
    State(state): State<SharedState>,
    Query(query): Query<ListJourneysQuery>,
) -> Result<Json<Vec<Journey>>, AppError> {
    let (limit, offset) = query.clamped();

    if let Some(cached) = state.journeys_cache.get(&(limit, offset)).await {
        return Ok(Json(cached));
    }

    let journeys = journey::service::list_public_journeys(&state.db, limit, offset).await?;
    state.journeys_cache.insert((limit, offset), journeys.clone()).await;
    Ok(Json(journeys))
}

/// GET /me/journeys — every journey the caller owns, including `draft`
/// (the one place drafts show up in a list rather than only being
/// fetchable one at a time by id via `GET /journeys/:id`).
async fn list_mine(
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
async fn create(
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
async fn get_journey(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(journey_id): Path<Uuid>,
) -> Result<Json<Journey>, AppError> {
    let journey = journey::service::get_journey(&state.db, journey_id, viewer.as_ref()).await?;
    Ok(Json(journey))
}

/// PATCH /journeys/:id — owner or moderator+ only; every field optional
/// (unset fields are left unchanged).
async fn update(
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
async fn list_checkpoints(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(journey_id): Path<Uuid>,
) -> Result<Json<Vec<Checkpoint>>, AppError> {
    let checkpoints =
        journey::service::list_checkpoints(&state.db, journey_id, viewer.as_ref()).await?;
    Ok(Json(checkpoints))
}

async fn create_checkpoint(
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
async fn list_posts(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(checkpoint_id): Path<Uuid>,
) -> Result<Json<Vec<Post>>, AppError> {
    let posts = journey::service::list_posts(&state.db, checkpoint_id, viewer.as_ref()).await?;
    Ok(Json(posts))
}

async fn create_post(
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

/// GET /journeys/:id/equipment — same visibility rule as checkpoints: the
/// owner/moderator sees every item, everyone else only published items on
/// a non-draft journey.
async fn list_equipment(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(journey_id): Path<Uuid>,
) -> Result<Json<Vec<Equipment>>, AppError> {
    let equipment = journey::service::list_equipment(&state.db, journey_id, viewer.as_ref()).await?;
    Ok(Json(equipment))
}

async fn create_equipment(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(journey_id): Path<Uuid>,
    Json(req): Json<CreateEquipmentRequest>,
) -> Result<(StatusCode, Json<Equipment>), AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let equipment = journey::service::create_equipment(&state.db, journey_id, &user, req).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "equipment_created",
        Some(json!({ "equipment_id": equipment.id, "journey_id": journey_id })),
    )
    .await
    {
        tracing::error!(?err, "failed to write equipment_created audit log");
    }

    Ok((StatusCode::CREATED, Json(equipment)))
}

/// GET /equipment-categories — public reference data, not journey-scoped,
/// no auth required.
async fn list_equipment_categories(
    State(state): State<SharedState>,
) -> Result<Json<Vec<EquipmentCategory>>, AppError> {
    let categories = journey::service::list_equipment_categories(&state.db).await?;
    Ok(Json(categories))
}

/// GET /journeys/:id/sponsors — same visibility rule as equipment: the
/// owner/moderator sees every sponsor, everyone else only published ones on
/// a non-draft journey.
async fn list_sponsors(
    State(state): State<SharedState>,
    OptionalAuthUser(viewer): OptionalAuthUser,
    Path(journey_id): Path<Uuid>,
) -> Result<Json<Vec<Sponsor>>, AppError> {
    let sponsors = journey::service::list_sponsors(&state.db, journey_id, viewer.as_ref()).await?;
    Ok(Json(sponsors))
}

async fn create_sponsor(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
    Path(journey_id): Path<Uuid>,
    Json(req): Json<CreateSponsorRequest>,
) -> Result<(StatusCode, Json<Sponsor>), AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let sponsor = journey::service::create_sponsor(&state.db, journey_id, &user, req).await?;

    if let Err(err) = audit::log(
        &state.db,
        Some(user.id),
        "sponsor_created",
        Some(json!({ "sponsor_id": sponsor.id, "journey_id": journey_id })),
    )
    .await
    {
        tracing::error!(?err, "failed to write sponsor_created audit log");
    }

    Ok((StatusCode::CREATED, Json(sponsor)))
}

#[derive(Debug, serde::Serialize)]
struct PresignUploadResponse {
    upload_url: String,
    public_url: String,
}

/// POST /uploads/presign-url — hands out a one-time presigned R2 PUT URL
/// plus the public URL the upload will be reachable at once done. Not
/// scoped to a specific journey/checkpoint (ownership only matters when
/// the resulting URL is actually saved, via update_journey/create_post) —
/// this only ever costs an R2 signature computation, no DB write, no
/// file bytes touch this server.
async fn presign_upload(
    State(state): State<SharedState>,
    AuthUser(user): AuthUser,
) -> Result<Json<PresignUploadResponse>, AppError> {
    if !user.role.can_use_app() {
        return Err(AppError::Forbidden);
    }

    let (upload_url, public_url) = state.r2.presign_upload();
    Ok(Json(PresignUploadResponse { upload_url, public_url }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use axum::body::Body;
    use axum::http::Request;
    use bikepackid_common::user::Role;
    use tower::ServiceExt;

    fn journey_create_body(title: &str) -> Body {
        Body::from(serde_json::to_vec(&serde_json::json!({ "title": title })).unwrap())
    }

    #[tokio::test]
    async fn create_journey_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let cookie = test_support::cookie_for(&state, user_id, Role::Viewer).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/journeys")
                    .header("cookie", cookie)
                    .header("content-type", "application/json")
                    .body(journey_create_body("Viewer's trip"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn create_journey_is_ok_for_creators() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let cookie = test_support::cookie_for(&state, user_id, Role::Creator).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/journeys")
                    .header("cookie", cookie)
                    .header("content-type", "application/json")
                    .body(journey_create_body("Creator's trip"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn get_draft_journey_is_not_found_for_an_unrelated_viewer() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let other_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let other_cookie = test_support::cookie_for(&state, other_id, Role::Creator).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "draft").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/journeys/{journey_id}"))
                    .header("cookie", other_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        test_support::delete_user(&state.db, owner_id).await;
        test_support::delete_user(&state.db, other_id).await;
    }

    #[tokio::test]
    async fn list_journeys_is_cached_and_misses_a_journey_inserted_after_the_first_call() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;

        let first_journey = test_support::insert_journey(&state.db, owner_id, "published").await;

        let app = router().with_state(state.clone());
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/journeys").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let journeys: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(journeys.iter().any(|j| j["id"] == first_journey.to_string()));

        // Inserted directly, bypassing the route that would populate the
        // cache — a real second write via PATCH/POST wouldn't invalidate
        // the cache either (TTL-only by design), but this skips needing a
        // second full create+publish round trip just to prove the point.
        let second_journey = test_support::insert_journey(&state.db, owner_id, "published").await;

        let response = app
            .oneshot(Request::builder().uri("/journeys").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let journeys: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(
            !journeys.iter().any(|j| j["id"] == second_journey.to_string()),
            "expected the cached response to still miss a journey inserted after the first call"
        );

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn list_mine_includes_the_callers_own_draft() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let owner_cookie = test_support::cookie_for(&state, owner_id, Role::Creator).await;

        let draft_id = test_support::insert_journey(&state.db, owner_id, "draft").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me/journeys")
                    .header("cookie", owner_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let journeys: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(journeys.iter().any(|j| j["id"] == draft_id.to_string()));

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn list_mine_without_a_cookie_is_unauthorized() {
        let state = test_support::app_state().await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me/journeys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn update_journey_is_forbidden_for_a_different_creator() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let other_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let other_cookie = test_support::cookie_for(&state, other_id, Role::Creator).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "planning").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/journeys/{journey_id}"))
                    .header("cookie", other_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&serde_json::json!({ "title": "hijacked" })).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        test_support::delete_user(&state.db, owner_id).await;
        test_support::delete_user(&state.db, other_id).await;
    }

    #[tokio::test]
    async fn update_journey_is_ok_for_a_moderator() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let moderator_id = test_support::insert_user_with_role(&state.db, Role::Moderator).await;
        let moderator_cookie = test_support::cookie_for(&state, moderator_id, Role::Moderator).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "planning").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/journeys/{journey_id}"))
                    .header("cookie", moderator_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&serde_json::json!({ "title": "moderated" })).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        test_support::delete_user(&state.db, owner_id).await;
        test_support::delete_user(&state.db, moderator_id).await;
    }

    #[tokio::test]
    async fn update_journey_is_forbidden_for_a_viewer_owner() {
        let state = test_support::app_state().await;
        // Owns the journey, but role has since been downgraded to viewer
        // (e.g. banned from the app) — ownership alone must not be enough.
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Viewer).await;
        let owner_cookie = test_support::cookie_for(&state, owner_id, Role::Viewer).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "planning").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/journeys/{journey_id}"))
                    .header("cookie", owner_cookie)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&serde_json::json!({ "title": "still mine?" })).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn list_checkpoints_is_public_for_a_published_journey() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;
        test_support::insert_checkpoint(&state.db, journey_id).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/journeys/{journey_id}/checkpoints"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let checkpoints: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(checkpoints.len(), 1);

        test_support::delete_user(&state.db, owner_id).await;
    }

    fn equipment_create_body(category_id: Uuid, name: &str) -> Body {
        Body::from(
            serde_json::to_vec(&serde_json::json!({ "category_id": category_id, "name": name }))
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn create_equipment_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let viewer_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let viewer_cookie = test_support::cookie_for(&state, viewer_id, Role::Viewer).await;
        let category_id = test_support::insert_equipment_category(&state.db).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/journeys/{journey_id}/equipment"))
                    .header("cookie", viewer_cookie)
                    .header("content-type", "application/json")
                    .body(equipment_create_body(category_id, "Tenda"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        test_support::delete_user(&state.db, owner_id).await;
        test_support::delete_user(&state.db, viewer_id).await;
    }

    #[tokio::test]
    async fn create_equipment_is_ok_for_creators() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let owner_cookie = test_support::cookie_for(&state, owner_id, Role::Creator).await;
        let category_id = test_support::insert_equipment_category(&state.db).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/journeys/{journey_id}/equipment"))
                    .header("cookie", owner_cookie)
                    .header("content-type", "application/json")
                    .body(equipment_create_body(category_id, "Tenda MSR"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "Tenda MSR");

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn list_equipment_is_public_for_a_published_journey() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let category_id = test_support::insert_equipment_category(&state.db).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;
        test_support::insert_equipment(&state.db, journey_id, category_id).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/journeys/{journey_id}/equipment"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let equipment: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(equipment.len(), 1);

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn list_equipment_categories_is_public_and_unauthenticated() {
        let state = test_support::app_state().await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/equipment-categories")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let categories: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(!categories.is_empty());
    }

    fn sponsor_create_body(name: &str) -> Body {
        Body::from(serde_json::to_vec(&serde_json::json!({ "name": name })).unwrap())
    }

    #[tokio::test]
    async fn create_sponsor_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let viewer_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let viewer_cookie = test_support::cookie_for(&state, viewer_id, Role::Viewer).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/journeys/{journey_id}/sponsors"))
                    .header("cookie", viewer_cookie)
                    .header("content-type", "application/json")
                    .body(sponsor_create_body("Acme Bikes"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        test_support::delete_user(&state.db, owner_id).await;
        test_support::delete_user(&state.db, viewer_id).await;
    }

    #[tokio::test]
    async fn create_sponsor_is_ok_for_creators() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let owner_cookie = test_support::cookie_for(&state, owner_id, Role::Creator).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/journeys/{journey_id}/sponsors"))
                    .header("cookie", owner_cookie)
                    .header("content-type", "application/json")
                    .body(sponsor_create_body("Acme Bikes"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "Acme Bikes");

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn list_sponsors_is_public_for_a_published_journey() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;

        let journey_id = test_support::insert_journey(&state.db, owner_id, "published").await;
        test_support::insert_sponsor(&state.db, journey_id).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/journeys/{journey_id}/sponsors"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let sponsors: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(sponsors.len(), 1);

        test_support::delete_user(&state.db, owner_id).await;
    }

    #[tokio::test]
    async fn presign_upload_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let cookie = test_support::cookie_for(&state, user_id, Role::Viewer).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/uploads/presign-url")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn presign_upload_returns_an_upload_and_public_url_for_creators() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let cookie = test_support::cookie_for(&state, user_id, Role::Creator).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/uploads/presign-url")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["upload_url"].as_str().unwrap().contains("X-Amz-Signature"));
        assert!(json["public_url"].as_str().unwrap().starts_with("http"));

        test_support::delete_user(&state.db, user_id).await;
    }
}
