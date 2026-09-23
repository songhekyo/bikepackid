pub mod auth;
pub mod health;
pub mod journey;
pub mod me;

use axum::{
    routing::{get, post},
    Router,
};
use tower_governor::{governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor, GovernorLayer};

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    // Bursts of 5 requests per IP, replenishing one every 2 seconds.
    // Scoped to just the Google OAuth endpoints — the only unauthenticated
    // routes worth protecting; /health needs to stay reachable for
    // liveness probes, and /me and /app/status already require a valid
    // session, which is self-limiting.
    //
    // SmartIpKeyExtractor reads x-forwarded-for/x-real-ip/forwarded first
    // and only falls back to the raw peer address (via ConnectInfo, wired
    // up in main.rs) if none of those are set. That's the right default
    // behind a reverse proxy (Railway/Render/Fly all set these), but it
    // means those headers must only be trusted because the proxy sets
    // them itself and strips any client-supplied copy — never expose this
    // server directly to the internet without a proxy doing that.
    let governor_conf = Box::leak(Box::new(
        GovernorConfigBuilder::default()
            .key_extractor(SmartIpKeyExtractor)
            .per_second(2)
            .burst_size(5)
            .finish()
            .expect("rate-limit config: burst_size and per_second must be non-zero"),
    ));

    let google_oauth_routes = Router::new()
        .route("/auth/google/login", get(auth::google_login))
        .route("/auth/google/callback", get(auth::google_callback))
        .layer(GovernorLayer {
            config: governor_conf,
        });

    Router::new()
        .route("/health", get(health::health))
        .route("/version", get(health::version))
        .merge(google_oauth_routes)
        .route("/auth/logout", post(auth::logout))
        .route("/auth/sign-out-everywhere", post(auth::sign_out_everywhere))
        .route("/me", get(me::me))
        .route("/app/status", get(me::app_status))
        .route("/me/journeys", get(journey::list_mine))
        .route("/journeys", get(journey::list).post(journey::create))
        .route("/journeys/:id", get(journey::get).patch(journey::update))
        .route(
            "/journeys/:id/checkpoints",
            get(journey::list_checkpoints).post(journey::create_checkpoint),
        )
        .route(
            "/checkpoints/:id/posts",
            get(journey::list_posts).post(journey::create_post),
        )
        .route("/uploads/presign-url", post(journey::presign_upload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        auth::{jwt, session},
        models::Role,
        test_support,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn cookie_for(state: &SharedState, user_id: uuid::Uuid, role: Role) -> String {
        let (session_id, expires_at) = session::create(&state.db, user_id).await.unwrap();
        let token = jwt::issue(user_id, role, session_id, expires_at, &state.config.jwt_secret);
        format!("session={token}")
    }

    #[tokio::test]
    async fn version_reports_a_git_sha() {
        let state = test_support::app_state().await;
        let app = router().with_state(state);

        let response = app
            .oneshot(Request::builder().uri("/version").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Local `cargo test` never sets GIT_SHA (only the Docker build
        // does), so "dev" is the expected value here — this just proves
        // the endpoint responds with the field, not a specific commit.
        assert_eq!(json["git_sha"], "dev");
    }

    #[tokio::test]
    async fn me_without_a_cookie_is_unauthorized() {
        let state = test_support::app_state().await;
        let app = router().with_state(state);

        let response = app
            .oneshot(Request::builder().uri("/me").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_with_a_valid_cookie_returns_the_user() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await;
        let cookie = cookie_for(&state, user_id, Role::Viewer).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn me_with_a_revoked_session_is_unauthorized() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await;

        let (session_id, expires_at) = session::create(&state.db, user_id).await.unwrap();
        let token = jwt::issue(
            user_id,
            Role::Viewer,
            session_id,
            expires_at,
            &state.config.jwt_secret,
        );
        session::revoke(&state.db, session_id).await.unwrap();

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("cookie", format!("session={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn sign_out_everywhere_revokes_every_session_for_the_caller() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await;
        let cookie_a = cookie_for(&state, user_id, Role::Viewer).await;
        let cookie_b = cookie_for(&state, user_id, Role::Viewer).await;

        let app = router().with_state(state.clone());

        // Both sessions work before signing out everywhere.
        for cookie in [&cookie_a, &cookie_b] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/me")
                        .header("cookie", cookie.clone())
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/sign-out-everywhere")
                    .header("cookie", cookie_a.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        // Both cookies are now unauthorized, including the one used to
        // call the endpoint itself.
        for cookie in [&cookie_a, &cookie_b] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/me")
                        .header("cookie", cookie.clone())
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn sign_out_everywhere_does_not_touch_another_users_session() {
        let state = test_support::app_state().await;
        let target_id = test_support::insert_user(&state.db).await;
        let other_id = test_support::insert_user(&state.db).await;
        let target_cookie = cookie_for(&state, target_id, Role::Viewer).await;
        let other_cookie = cookie_for(&state, other_id, Role::Viewer).await;

        let app = router().with_state(state.clone());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/auth/sign-out-everywhere")
                    .header("cookie", target_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("cookie", other_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        test_support::delete_user(&state.db, target_id).await;
        test_support::delete_user(&state.db, other_id).await;
    }

    #[tokio::test]
    async fn app_status_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let cookie = cookie_for(&state, user_id, Role::Viewer).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/app/status")
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
    async fn app_status_is_ok_for_creators() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let cookie = cookie_for(&state, user_id, Role::Creator).await;

        let app = router().with_state(state.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/app/status")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        test_support::delete_user(&state.db, user_id).await;
    }

    #[tokio::test]
    async fn google_login_is_rate_limited_per_ip() {
        let state = test_support::app_state().await;
        let app = router().with_state(state);

        let hit = |app: Router<()>, ip: &'static str| {
            let app = app.clone();
            async move {
                app.oneshot(
                    Request::builder()
                        .uri("/auth/google/login")
                        .header("x-forwarded-for", ip)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
                .status()
            }
        };

        // Burst size is 5: the first 5 requests from the same IP succeed
        // (a redirect to Google), the 6th is throttled.
        for _ in 0..5 {
            assert_eq!(hit(app.clone(), "203.0.113.10").await, StatusCode::SEE_OTHER);
        }
        assert_eq!(
            hit(app.clone(), "203.0.113.10").await,
            StatusCode::TOO_MANY_REQUESTS
        );

        // A different IP has its own, untouched quota.
        assert_eq!(
            hit(app.clone(), "203.0.113.20").await,
            StatusCode::SEE_OTHER
        );
    }

    /// Pulls the `state` value out of the Location header from
    /// `/auth/google/login`'s redirect, so a callback test can present a
    /// `state` the server actually recognizes as a real, in-flight login
    /// attempt (rather than an arbitrary string).
    async fn start_login_and_get_state(app: &Router<()>) -> String {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/auth/google/login")
                    .header("x-forwarded-for", "198.51.100.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let location = response
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();

        location
            .split("state=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("login redirect must carry a state param")
            .to_string()
    }

    #[tokio::test]
    async fn callback_forwards_googles_error_unlabeled_for_a_real_login_attempt() {
        let state = test_support::app_state().await;
        let app = router().with_state(state.clone());

        let csrf_state = start_login_and_get_state(&app).await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/auth/google/callback?state={csrf_state}&error=access_denied"
                    ))
                    // The rate limiter's key extractor needs an IP from
                    // somewhere; .oneshot() has no real peer address, so
                    // supply one via header like the rate-limit test does.
                    .header("x-forwarded-for", "198.51.100.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        // Relayed as-is (not renamed to something like "cancelled") so the
        // frontend decides what each Google error code means.
        assert!(
            location.ends_with("?error=access_denied"),
            "expected the raw google error forwarded in the redirect, got {location}"
        );
    }

    #[tokio::test]
    async fn callback_rejects_an_error_param_with_no_matching_login_attempt() {
        let state = test_support::app_state().await;
        let app = router().with_state(state);

        // No prior call to /auth/google/login, so this `state` was never
        // issued by us — an `error` param must not get reflected back for
        // an attempt we don't recognize.
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/auth/google/callback?state=not-a-real-attempt&error=access_denied")
                    .header("x-forwarded-for", "198.51.100.2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    fn journey_create_body(title: &str) -> Body {
        Body::from(
            serde_json::to_vec(&serde_json::json!({ "title": title }))
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn create_journey_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let cookie = cookie_for(&state, user_id, Role::Viewer).await;

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
        let cookie = cookie_for(&state, user_id, Role::Creator).await;

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
        let other_cookie = cookie_for(&state, other_id, Role::Creator).await;

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
    async fn list_mine_includes_the_callers_own_draft() {
        let state = test_support::app_state().await;
        let owner_id = test_support::insert_user_with_role(&state.db, Role::Creator).await;
        let owner_cookie = cookie_for(&state, owner_id, Role::Creator).await;

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
        let other_cookie = cookie_for(&state, other_id, Role::Creator).await;

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
        let moderator_cookie = cookie_for(&state, moderator_id, Role::Moderator).await;

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
        let owner_cookie = cookie_for(&state, owner_id, Role::Viewer).await;

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

    #[tokio::test]
    async fn presign_upload_is_forbidden_for_viewers() {
        let state = test_support::app_state().await;
        let user_id = test_support::insert_user(&state.db).await; // defaults to viewer
        let cookie = cookie_for(&state, user_id, Role::Viewer).await;

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
        let cookie = cookie_for(&state, user_id, Role::Creator).await;

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
