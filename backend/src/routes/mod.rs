pub mod auth;
pub mod health;
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
        .merge(google_oauth_routes)
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(me::me))
        .route("/app/status", get(me::app_status))
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
}
