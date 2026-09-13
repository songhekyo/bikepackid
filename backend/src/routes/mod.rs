pub mod auth;
pub mod health;
pub mod me;

use axum::{
    routing::{get, post},
    Router,
};

use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/health", get(health::health))
        .route("/auth/google/login", get(auth::google_login))
        .route("/auth/google/callback", get(auth::google_callback))
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
}
