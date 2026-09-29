use bikepackid_common::{error::AppError, user::User};
use sqlx::PgPool;
use uuid::Uuid;

use super::{
    Checkpoint, CreateCheckpointRequest, CreateJourneyRequest, CreatePostRequest, Journey,
    JourneyStatus, Post, UpdateJourneyRequest,
};

/// The one place "can this user edit this journey" is decided — every
/// write path calls through here instead of repeating the check, so a
/// future collaborator feature (see docs/SYSTEM_DESIGN.md) only needs this
/// function's body changed, not every handler that currently calls it.
fn user_can_edit_journey(journey: &Journey, user: &User) -> bool {
    journey.user_id == user.id || user.role.can_moderate()
}

#[tracing::instrument(skip(pool))]
async fn fetch_journey(pool: &PgPool, journey_id: Uuid) -> Result<Option<Journey>, AppError> {
    let journey = sqlx::query_as::<_, Journey>("SELECT * FROM journeys WHERE id = $1")
        .bind(journey_id)
        .fetch_optional(pool)
        .await?;

    Ok(journey)
}

#[tracing::instrument(skip(pool))]
async fn fetch_checkpoint(pool: &PgPool, checkpoint_id: Uuid) -> Result<Option<Checkpoint>, AppError> {
    let checkpoint = sqlx::query_as::<_, Checkpoint>("SELECT * FROM checkpoints WHERE id = $1")
        .bind(checkpoint_id)
        .fetch_optional(pool)
        .await?;

    Ok(checkpoint)
}

#[tracing::instrument(skip(pool))]
async fn fetch_post(pool: &PgPool, post_id: Uuid) -> Result<Option<Post>, AppError> {
    let post = sqlx::query_as::<_, Post>("SELECT * FROM posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(pool)
        .await?;

    Ok(post)
}

#[tracing::instrument(skip(pool, req))]
pub async fn create_journey(
    pool: &PgPool,
    user_id: Uuid,
    req: CreateJourneyRequest,
) -> Result<Journey, AppError> {
    let journey = sqlx::query_as::<_, Journey>(
        r#"
        INSERT INTO journeys (user_id, title, description, start_date, end_date, cover_image)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(req.title)
    .bind(req.description)
    .bind(req.start_date)
    .bind(req.end_date)
    .bind(req.cover_image)
    .fetch_one(pool)
    .await?;

    Ok(journey)
}

/// Fetches a journey, enforcing the same draft-visibility rule
/// `visible_checkpoints`/`visible_posts` encode for their own tables: a
/// `draft` journey is only visible to its owner or a moderator+. Returns
/// `NotFound` (not `Forbidden`) when invisible, so a draft's existence
/// isn't leaked to anyone who isn't allowed to see it.
#[tracing::instrument(skip(pool, viewer))]
pub async fn get_journey(
    pool: &PgPool,
    journey_id: Uuid,
    viewer: Option<&User>,
) -> Result<Journey, AppError> {
    let journey = fetch_journey(pool, journey_id).await?.ok_or(AppError::NotFound)?;

    let visible = journey.status != JourneyStatus::Draft
        || viewer.is_some_and(|u| user_can_edit_journey(&journey, u));

    if !visible {
        return Err(AppError::NotFound);
    }

    Ok(journey)
}

#[tracing::instrument(skip(pool))]
pub async fn list_public_journeys(
    pool: &PgPool,
    limit: i64,
    offset: i64,
) -> Result<Vec<Journey>, AppError> {
    let journeys = sqlx::query_as::<_, Journey>(
        r#"
        SELECT * FROM journeys
        WHERE status != 'draft'
        ORDER BY created_at DESC
        LIMIT $1 OFFSET $2
        "#,
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(journeys)
}

/// All journeys owned by `user_id`, regardless of status — unlike
/// `list_public_journeys`, this is the one place `draft` journeys show up
/// in a list rather than only being fetchable one at a time by id.
#[tracing::instrument(skip(pool))]
pub async fn list_my_journeys(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<Journey>, AppError> {
    let journeys = sqlx::query_as::<_, Journey>(
        r#"
        SELECT * FROM journeys
        WHERE user_id = $1
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(journeys)
}

#[tracing::instrument(skip(pool, user, req), fields(user_id = %user.id))]
pub async fn update_journey(
    pool: &PgPool,
    journey_id: Uuid,
    user: &User,
    req: UpdateJourneyRequest,
) -> Result<Journey, AppError> {
    let journey = fetch_journey(pool, journey_id).await?.ok_or(AppError::NotFound)?;

    if !user_can_edit_journey(&journey, user) {
        return Err(AppError::Forbidden);
    }

    let updated = sqlx::query_as::<_, Journey>(
        r#"
        UPDATE journeys SET
            title = COALESCE($1, title),
            description = COALESCE($2, description),
            status = COALESCE($3, status),
            start_date = COALESCE($4, start_date),
            end_date = COALESCE($5, end_date),
            cover_image = COALESCE($6, cover_image),
            start_lat = COALESCE($7, start_lat),
            start_lng = COALESCE($8, start_lng),
            end_lat = COALESCE($9, end_lat),
            end_lng = COALESCE($10, end_lng),
            seeking_sponsor = COALESCE($11, seeking_sponsor),
            donation_url = COALESCE($12, donation_url),
            updated_at = now()
        WHERE id = $13
        RETURNING *
        "#,
    )
    .bind(req.title)
    .bind(req.description)
    .bind(req.status)
    .bind(req.start_date)
    .bind(req.end_date)
    .bind(req.cover_image)
    .bind(req.start_lat)
    .bind(req.start_lng)
    .bind(req.end_lat)
    .bind(req.end_lng)
    .bind(req.seeking_sponsor)
    .bind(req.donation_url)
    .bind(journey_id)
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

#[tracing::instrument(skip(pool, user, req), fields(user_id = %user.id))]
pub async fn create_checkpoint(
    pool: &PgPool,
    journey_id: Uuid,
    user: &User,
    req: CreateCheckpointRequest,
) -> Result<Checkpoint, AppError> {
    let journey = fetch_journey(pool, journey_id).await?.ok_or(AppError::NotFound)?;

    if !user_can_edit_journey(&journey, user) {
        return Err(AppError::Forbidden);
    }

    let trigger_type = req.trigger_type.unwrap_or_else(|| "manual".to_string());

    // ON CONFLICT DO UPDATE (a no-op update to the conflicting row) rather
    // than DO NOTHING: DO NOTHING returns no row on conflict, but a client
    // retrying a dropped-connection sync with the same `id` needs the
    // checkpoint back, not an empty result — this is what makes the
    // client-suppliable `id` actually idempotent (see CreateCheckpointRequest::id).
    let checkpoint = sqlx::query_as::<_, Checkpoint>(
        r#"
        INSERT INTO checkpoints (id, journey_id, lat, lng, captured_at, title, trigger_type)
        VALUES (COALESCE($1, gen_random_uuid()), $2, $3, $4, $5, $6, $7)
        ON CONFLICT (id) DO UPDATE SET id = checkpoints.id
        RETURNING *
        "#,
    )
    .bind(req.id)
    .bind(journey_id)
    .bind(req.lat)
    .bind(req.lng)
    .bind(req.captured_at)
    .bind(req.title)
    .bind(trigger_type)
    .fetch_one(pool)
    .await?;

    Ok(checkpoint)
}

#[tracing::instrument(skip(pool, viewer))]
pub async fn list_checkpoints(
    pool: &PgPool,
    journey_id: Uuid,
    viewer: Option<&User>,
) -> Result<Vec<Checkpoint>, AppError> {
    let journey = get_journey(pool, journey_id, viewer).await?;

    let checkpoints = if viewer.is_some_and(|u| user_can_edit_journey(&journey, u)) {
        sqlx::query_as::<_, Checkpoint>(
            "SELECT * FROM checkpoints WHERE journey_id = $1 ORDER BY captured_at",
        )
        .bind(journey_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Checkpoint>(
            "SELECT * FROM visible_checkpoints WHERE journey_id = $1 ORDER BY captured_at",
        )
        .bind(journey_id)
        .fetch_all(pool)
        .await?
    };

    Ok(checkpoints)
}

#[tracing::instrument(skip(pool, user, req), fields(user_id = %user.id))]
pub async fn create_post(
    pool: &PgPool,
    checkpoint_id: Uuid,
    user: &User,
    req: CreatePostRequest,
) -> Result<Post, AppError> {
    let checkpoint = fetch_checkpoint(pool, checkpoint_id).await?.ok_or(AppError::NotFound)?;
    let journey = fetch_journey(pool, checkpoint.journey_id).await?.ok_or(AppError::NotFound)?;

    if !user_can_edit_journey(&journey, user) {
        return Err(AppError::Forbidden);
    }

    if let Some(parent_id) = req.parent_post_id {
        let parent = fetch_post(pool, parent_id).await?.ok_or_else(|| {
            AppError::BadRequest("parent post must belong to the same checkpoint".to_string())
        })?;
        if parent.checkpoint_id != checkpoint_id {
            return Err(AppError::BadRequest(
                "parent post must belong to the same checkpoint".to_string(),
            ));
        }
    }

    let post = sqlx::query_as::<_, Post>(
        r#"
        INSERT INTO posts (checkpoint_id, type, body, media_url, parent_post_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(checkpoint_id)
    .bind(req.r#type)
    .bind(req.body)
    .bind(req.media_url)
    .bind(req.parent_post_id)
    .fetch_one(pool)
    .await?;

    Ok(post)
}

#[tracing::instrument(skip(pool, viewer))]
pub async fn list_posts(
    pool: &PgPool,
    checkpoint_id: Uuid,
    viewer: Option<&User>,
) -> Result<Vec<Post>, AppError> {
    let checkpoint = fetch_checkpoint(pool, checkpoint_id).await?.ok_or(AppError::NotFound)?;
    let journey = get_journey(pool, checkpoint.journey_id, viewer).await?;

    let posts = if viewer.is_some_and(|u| user_can_edit_journey(&journey, u)) {
        sqlx::query_as::<_, Post>("SELECT * FROM posts WHERE checkpoint_id = $1 ORDER BY created_at")
            .bind(checkpoint_id)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query_as::<_, Post>(
            "SELECT * FROM visible_posts WHERE checkpoint_id = $1 ORDER BY created_at",
        )
        .bind(checkpoint_id)
        .fetch_all(pool)
        .await?
    };

    Ok(posts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journey::PostType;
    use crate::test_support;
    use bikepackid_common::user::Role;

    fn journey_request(title: &str) -> CreateJourneyRequest {
        CreateJourneyRequest {
            title: title.to_string(),
            description: None,
            start_date: None,
            end_date: None,
            cover_image: None,
        }
    }

    #[tokio::test]
    async fn list_my_journeys_includes_draft_and_excludes_other_users() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let other_id = test_support::insert_user(&pool).await;

        let draft = create_journey(&pool, user_id, journey_request("Draft")).await.unwrap();
        let published = test_support::insert_journey(&pool, user_id, "published").await;
        let other_journey = test_support::insert_journey(&pool, other_id, "published").await;

        let mine = list_my_journeys(&pool, user_id, 50, 0).await.unwrap();
        let mine_ids: Vec<_> = mine.iter().map(|j| j.id).collect();

        assert!(mine_ids.contains(&draft.id), "draft must show up in the owner's own list");
        assert!(mine_ids.contains(&published));
        assert!(!mine_ids.contains(&other_journey));

        test_support::delete_user(&pool, user_id).await;
        test_support::delete_user(&pool, other_id).await;
    }

    #[tokio::test]
    async fn create_and_get_journey_roundtrip() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let created = create_journey(&pool, user_id, journey_request("Trip A")).await.unwrap();
        assert_eq!(created.status, JourneyStatus::Draft);

        let fetched = get_journey(&pool, created.id, None).await;
        assert!(matches!(fetched, Err(AppError::NotFound)), "draft must not be visible to an anonymous viewer");

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn owner_can_see_their_own_draft() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let user = test_support::fetch_user(&pool, user_id).await;

        let created = create_journey(&pool, user_id, journey_request("Trip B")).await.unwrap();
        let fetched = get_journey(&pool, created.id, Some(&user)).await.unwrap();
        assert_eq!(fetched.id, created.id);

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn list_public_journeys_excludes_draft_includes_planning_published_archived() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;

        let draft = create_journey(&pool, user_id, journey_request("Draft")).await.unwrap();
        let planning = test_support::insert_journey(&pool, user_id, "planning").await;
        let published = test_support::insert_journey(&pool, user_id, "published").await;
        let archived = test_support::insert_journey(&pool, user_id, "archived").await;

        let listed = list_public_journeys(&pool, 50, 0).await.unwrap();
        let listed_ids: Vec<_> = listed.iter().map(|j| j.id).collect();

        assert!(!listed_ids.contains(&draft.id));
        assert!(listed_ids.contains(&planning));
        assert!(listed_ids.contains(&published));
        assert!(listed_ids.contains(&archived));

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn published_checkpoint_under_draft_journey_is_not_publicly_visible() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let user = test_support::fetch_user(&pool, user_id).await;

        let journey = create_journey(&pool, user_id, journey_request("Draft with checkpoint")).await.unwrap();
        let checkpoint = create_checkpoint(
            &pool,
            journey.id,
            &user,
            CreateCheckpointRequest {
                id: None,
                lat: 1.0,
                lng: 2.0,
                captured_at: chrono::Utc::now(),
                title: None,
                trigger_type: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(checkpoint.journey_id, journey.id);

        // Anonymous viewer must not see the checkpoint (journey is still draft) ...
        let public_result = list_checkpoints(&pool, journey.id, None).await;
        assert!(matches!(public_result, Err(AppError::NotFound)));

        // ... but the owner still can.
        let owner_result = list_checkpoints(&pool, journey.id, Some(&user)).await.unwrap();
        assert_eq!(owner_result.len(), 1);

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn update_journey_rejects_non_owner_non_moderator() {
        let pool = test_support::pool().await;
        let owner_id = test_support::insert_user(&pool).await;
        let other_id = test_support::insert_user_with_role(&pool, Role::Creator).await;
        let other = test_support::fetch_user(&pool, other_id).await;

        let journey = create_journey(&pool, owner_id, journey_request("Owned")).await.unwrap();

        let result = update_journey(
            &pool,
            journey.id,
            &other,
            UpdateJourneyRequest { title: Some("hijacked".to_string()), ..Default::default() },
        )
        .await;

        assert!(matches!(result, Err(AppError::Forbidden)));

        test_support::delete_user(&pool, owner_id).await;
        test_support::delete_user(&pool, other_id).await;
    }

    #[tokio::test]
    async fn update_journey_allows_moderator_even_when_not_owner() {
        let pool = test_support::pool().await;
        let owner_id = test_support::insert_user(&pool).await;
        let moderator_id = test_support::insert_user_with_role(&pool, Role::Moderator).await;
        let moderator = test_support::fetch_user(&pool, moderator_id).await;

        let journey = create_journey(&pool, owner_id, journey_request("Owned")).await.unwrap();

        let updated = update_journey(
            &pool,
            journey.id,
            &moderator,
            UpdateJourneyRequest { title: Some("moderated".to_string()), ..Default::default() },
        )
        .await
        .unwrap();

        assert_eq!(updated.title, "moderated");

        test_support::delete_user(&pool, owner_id).await;
        test_support::delete_user(&pool, moderator_id).await;
    }

    #[tokio::test]
    async fn create_post_requires_a_photo_type_or_other_valid_type() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let user = test_support::fetch_user(&pool, user_id).await;

        let journey = create_journey(&pool, user_id, journey_request("Journey with post")).await.unwrap();
        let checkpoint = create_checkpoint(
            &pool,
            journey.id,
            &user,
            CreateCheckpointRequest {
                id: None,
                lat: 1.0,
                lng: 2.0,
                captured_at: chrono::Utc::now(),
                title: None,
                trigger_type: None,
            },
        )
        .await
        .unwrap();

        let post = create_post(
            &pool,
            checkpoint.id,
            &user,
            CreatePostRequest {
                r#type: PostType::Text,
                body: Some("hello".to_string()),
                media_url: None,
                parent_post_id: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(post.checkpoint_id, checkpoint.id);

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn create_checkpoint_with_the_same_id_twice_is_idempotent() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let user = test_support::fetch_user(&pool, user_id).await;

        let journey = create_journey(&pool, user_id, journey_request("Retry")).await.unwrap();
        let shared_id = Uuid::new_v4();

        let req = || CreateCheckpointRequest {
            id: Some(shared_id),
            lat: 1.0,
            lng: 2.0,
            captured_at: chrono::Utc::now(),
            title: None,
            trigger_type: None,
        };

        let first = create_checkpoint(&pool, journey.id, &user, req()).await.unwrap();
        let retried = create_checkpoint(&pool, journey.id, &user, req()).await.unwrap();

        assert_eq!(first.id, shared_id);
        assert_eq!(retried.id, shared_id);

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn update_journey_to_planning_without_coordinates_is_a_bad_request() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let user = test_support::fetch_user(&pool, user_id).await;

        let journey = create_journey(&pool, user_id, journey_request("Missing coords")).await.unwrap();

        let result = update_journey(
            &pool,
            journey.id,
            &user,
            UpdateJourneyRequest { status: Some(JourneyStatus::Planning), ..Default::default() },
        )
        .await;

        assert!(matches!(result, Err(AppError::BadRequest(_))), "expected BadRequest, got {result:?}");

        test_support::delete_user(&pool, user_id).await;
    }

    #[tokio::test]
    async fn create_post_rejects_a_parent_post_from_a_different_checkpoint() {
        let pool = test_support::pool().await;
        let user_id = test_support::insert_user(&pool).await;
        let user = test_support::fetch_user(&pool, user_id).await;

        let journey = create_journey(&pool, user_id, journey_request("Two checkpoints")).await.unwrap();

        let checkpoint_request = || CreateCheckpointRequest {
            id: None,
            lat: 1.0,
            lng: 2.0,
            captured_at: chrono::Utc::now(),
            title: None,
            trigger_type: None,
        };
        let checkpoint_a = create_checkpoint(&pool, journey.id, &user, checkpoint_request()).await.unwrap();
        let checkpoint_b = create_checkpoint(&pool, journey.id, &user, checkpoint_request()).await.unwrap();

        let post_under_a = create_post(
            &pool,
            checkpoint_a.id,
            &user,
            CreatePostRequest { r#type: PostType::Text, body: Some("a".to_string()), media_url: None, parent_post_id: None },
        )
        .await
        .unwrap();

        let result = create_post(
            &pool,
            checkpoint_b.id,
            &user,
            CreatePostRequest {
                r#type: PostType::Text,
                body: Some("b".to_string()),
                media_url: None,
                parent_post_id: Some(post_under_a.id),
            },
        )
        .await;

        assert!(matches!(result, Err(AppError::BadRequest(_))), "expected BadRequest, got {result:?}");

        test_support::delete_user(&pool, user_id).await;
    }
}
