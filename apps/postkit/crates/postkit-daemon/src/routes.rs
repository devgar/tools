use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{Json, Response},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use postkit_core::Provider;
use postkit_store::{ListFilters, ScheduledPost, Store};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

pub struct AppState {
    pub store: Store,
    pub providers: Arc<HashMap<String, Arc<dyn Provider>>>,
    /// None → sin autenticación (dev local).
    pub api_key: Option<String>,
}

pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/schedule", post(schedule_post))
        .route("/scheduled", get(list_scheduled))
        .route("/scheduled/{id}", get(get_scheduled).delete(cancel_scheduled))
        .route("/scheduled/{id}/retry", post(retry_scheduled))
        .layer(middleware::from_fn_with_state(state.clone(), auth));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .with_state(state)
}

// ─── Auth middleware ──────────────────────────────────────────────────────────

async fn auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, &'static str)> {
    if let Some(expected) = &state.api_key {
        let provided = req
            .headers()
            .get("X-Api-Key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if provided != expected {
            return Err((StatusCode::UNAUTHORIZED, "API key inválida o ausente"));
        }
    }
    Ok(next.run(req).await)
}

// ─── GET /health ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok", version: env!("CARGO_PKG_VERSION") })
}

// ─── POST /schedule ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ScheduleBody {
    pub account_id: String,
    pub source_post: postkit_core::SourcePost,
    pub scheduled_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct IdResponse {
    id: i64,
}

async fn schedule_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ScheduleBody>,
) -> Result<Json<IdResponse>, (StatusCode, String)> {
    let provider = state
        .providers
        .get(&body.account_id)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("cuenta desconocida: {}", body.account_id)))?;

    let provider_str = format!("{:?}", provider.kind()).to_lowercase();
    let source_json = serde_json::to_string(&body.source_post)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let id = state
        .store
        .schedule(&body.account_id, &provider_str, &source_json, body.scheduled_at)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(IdResponse { id }))
}

// ─── GET /scheduled ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListQuery {
    account_id: Option<String>,
    provider: Option<String>,
    status: Option<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_scheduled(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ScheduledPost>>, (StatusCode, String)> {
    let posts = state
        .store
        .list(&ListFilters {
            account_id: q.account_id,
            provider: q.provider,
            status: q.status,
            from: q.from,
            to: q.to,
            limit: q.limit,
            offset: q.offset,
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(posts))
}

// ─── GET /scheduled/:id ──────────────────────────────────────────────────────

async fn get_scheduled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ScheduledPost>, (StatusCode, String)> {
    state
        .store
        .get_by_id(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("post {id} no encontrado")))
}

// ─── DELETE /scheduled/:id ───────────────────────────────────────────────────

async fn cancel_scheduled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let ok = state
        .store
        .cancel(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("post {id} no encontrado o no está en pending")))
    }
}

// ─── POST /scheduled/:id/retry ───────────────────────────────────────────────

async fn retry_scheduled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let ok = state
        .store
        .retry(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("post {id} no encontrado o no está en failed")))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use postkit_core::{
        AccountInfo, Capabilities, PreparedPost, Provider, ProviderKind, PublishResult, SourcePost,
    };
    use postkit_store::Store;
    use tower::ServiceExt;

    struct MockProvider;

    #[async_trait]
    impl Provider for MockProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Bluesky }
        fn account_id(&self) -> &str { "test" }
        fn capabilities(&self) -> Capabilities {
            Capabilities { max_text_graphemes: 300, max_media: 4, supports_threads: false, supports_alt_text: true }
        }
        async fn verify(&self) -> anyhow::Result<AccountInfo> { unimplemented!() }
        fn compose(&self, _: &SourcePost) -> anyhow::Result<PreparedPost> { unimplemented!() }
        async fn execute(&self, _: &PreparedPost) -> anyhow::Result<PublishResult> { unimplemented!() }
    }

    async fn mem_state(api_key: Option<&str>) -> Arc<AppState> {
        let store = Store::open(":memory:").await.unwrap();
        Arc::new(AppState {
            store,
            providers: Arc::new(HashMap::new()),
            api_key: api_key.map(str::to_string),
        })
    }

    async fn mem_state_with_provider() -> Arc<AppState> {
        let store = Store::open(":memory:").await.unwrap();
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        providers.insert("test".to_string(), Arc::new(MockProvider));
        Arc::new(AppState { store, providers: Arc::new(providers), api_key: None })
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let resp = router(mem_state(None).await)
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_rejects_missing_key() {
        let resp = router(mem_state(Some("secret")).await)
            .oneshot(Request::get("/scheduled").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_rejects_wrong_key() {
        let resp = router(mem_state(Some("secret")).await)
            .oneshot(
                Request::get("/scheduled").header("X-Api-Key", "wrong").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_accepts_correct_key() {
        let resp = router(mem_state(Some("secret")).await)
            .oneshot(
                Request::get("/scheduled")
                    .header("X-Api-Key", "secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn list_scheduled_returns_empty() {
        let resp = router(mem_state(None).await)
            .oneshot(Request::get("/scheduled").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&*bytes, b"[]");
    }

    #[tokio::test]
    async fn get_scheduled_not_found() {
        let resp = router(mem_state(None).await)
            .oneshot(Request::get("/scheduled/999").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn cancel_not_found() {
        let resp = router(mem_state(None).await)
            .oneshot(Request::delete("/scheduled/999").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn retry_not_found() {
        let resp = router(mem_state(None).await)
            .oneshot(Request::post("/scheduled/999/retry").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn schedule_unknown_account_returns_bad_request() {
        let body = serde_json::json!({
            "account_id": "ghost",
            "scheduled_at": "2026-04-21T10:00:00Z",
            "source_post": {"text": "hi", "media": [], "hashtags": []}
        });
        let resp = router(mem_state(None).await)
            .oneshot(
                Request::post("/schedule")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn schedule_creates_post_returns_id() {
        let body = serde_json::json!({
            "account_id": "test",
            "scheduled_at": "2026-04-21T10:00:00Z",
            "source_post": {"text": "hi", "media": [], "hashtags": []}
        });
        let resp = router(mem_state_with_provider().await)
            .oneshot(
                Request::post("/schedule")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["id"], 1);
    }
}
