use std::{collections::HashMap, sync::Arc};

use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use postkit_core::Provider;
use postkit_store::{ListFilters, ScheduledPost, Store};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, OpenApi, ToSchema};

use crate::queue::AnyQueue;

pub struct AppState {
    pub store: Store,
    pub providers: Arc<HashMap<String, Arc<dyn Provider>>>,
    /// None → authentication disabled (local dev).
    pub api_key: Option<String>,
    pub queue: AnyQueue,
}

// ─── OpenAPI ─────────────────────────────────────────────────────────────────

struct ApiKeyAuth;

impl utoipa::Modify for ApiKeyAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(c) = openapi.components.as_mut() {
            c.add_security_scheme(
                "api_key",
                utoipa::openapi::security::SecurityScheme::ApiKey(
                    utoipa::openapi::security::ApiKey::Header(
                        utoipa::openapi::security::ApiKeyValue::new("X-Api-Key"),
                    ),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(
        health,
        openapi_spec,
        schedule_post,
        list_scheduled,
        get_scheduled,
        cancel_scheduled,
        update_scheduled,
        retry_scheduled,
    ),
    components(schemas(
        Health,
        ScheduleBody,
        IdResponse,
        UpdateBody,
        postkit_store::ScheduledPost,
    )),
    modifiers(&ApiKeyAuth),
    info(title = "postkit API", version = env!("CARGO_PKG_VERSION")),
)]
struct ApiDoc;

#[utoipa::path(
    get,
    path = "/openapi.json",
    responses(
        (status = 200, description = "OpenAPI spec (JSON)")
    ),
    tag = "system"
)]
async fn openapi_spec() -> impl IntoResponse { Json(ApiDoc::openapi()) }

pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/schedule", post(schedule_post))
        .route("/scheduled", get(list_scheduled))
        .route(
            "/scheduled/{id}",
            get(get_scheduled)
                .delete(cancel_scheduled)
                .put(update_scheduled),
        )
        .route("/scheduled/{id}/retry", post(retry_scheduled))
        .layer(middleware::from_fn_with_state(state.clone(), auth));

    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi_spec))
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
            return Err((StatusCode::UNAUTHORIZED, "invalid or missing API key"));
        }
    }
    Ok(next.run(req).await)
}

// ─── GET /health ─────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema)]
struct Health {
    status: &'static str,
    version: &'static str,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = Health)
    ),
    tag = "system"
)]
async fn health() -> Json<Health> {
    Json(Health { status: "ok", version: env!("CARGO_PKG_VERSION") })
}

// ─── POST /schedule ──────────────────────────────────────────────────────────

#[derive(Deserialize, ToSchema)]
pub struct ScheduleBody {
    pub account_id: String,
    #[schema(value_type = Object)]
    pub source_post: postkit_core::SourcePost,
    pub scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
struct IdResponse {
    id: i64,
}

#[utoipa::path(
    post,
    path = "/schedule",
    request_body = ScheduleBody,
    responses(
        (status = 200, description = "Post scheduled or draft created", body = IdResponse),
        (status = 400, description = "Unknown account"),
        (status = 500, description = "Internal error"),
    ),
    security(("api_key" = [])),
    tag = "posts"
)]
async fn schedule_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ScheduleBody>,
) -> Result<Json<IdResponse>, (StatusCode, String)> {
    let provider = state.providers.get(&body.account_id).ok_or_else(|| {
        (StatusCode::BAD_REQUEST, format!("unknown account: {}", body.account_id))
    })?;

    let provider_str = format!("{:?}", provider.kind()).to_lowercase();
    let source_json = serde_json::to_string(&body.source_post)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let id = match body.scheduled_at {
        Some(at) => {
            let id = state
                .store
                .schedule(&body.account_id, &provider_str, &source_json, at)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if let Err(e) = state.queue.push(id, at.timestamp()).await {
                tracing::warn!(id, "queue: {e}");
            }
            id
        }
        None => state
            .store
            .create_draft(&body.account_id, &provider_str, &source_json)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    Ok(Json(IdResponse { id }))
}

// ─── GET /scheduled ──────────────────────────────────────────────────────────

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct ListQuery {
    account_id: Option<String>,
    provider: Option<String>,
    status: Option<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/scheduled",
    params(ListQuery),
    responses(
        (status = 200, description = "List of scheduled posts", body = Vec<ScheduledPost>),
        (status = 500, description = "Internal error"),
    ),
    security(("api_key" = [])),
    tag = "posts"
)]
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

#[utoipa::path(
    get,
    path = "/scheduled/{id}",
    params(("id" = i64, Path, description = "Post ID")),
    responses(
        (status = 200, description = "Scheduled post", body = ScheduledPost),
        (status = 404, description = "Post not found"),
        (status = 500, description = "Internal error"),
    ),
    security(("api_key" = [])),
    tag = "posts"
)]
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
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("post {id} not found")))
}

// ─── DELETE /scheduled/:id ───────────────────────────────────────────────────

#[utoipa::path(
    delete,
    path = "/scheduled/{id}",
    params(("id" = i64, Path, description = "Post ID")),
    responses(
        (status = 204, description = "Post cancelled"),
        (status = 404, description = "Post not found or not cancellable"),
        (status = 500, description = "Internal error"),
    ),
    security(("api_key" = [])),
    tag = "posts"
)]
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
        if let Err(e) = state.queue.remove(id).await {
            tracing::warn!(id, "queue: {e}");
        }
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("post {id} not found or not in pending state")))
    }
}

// ─── PUT /scheduled/:id ──────────────────────────────────────────────────────

#[derive(Deserialize, ToSchema)]
struct UpdateBody {
    #[schema(value_type = Object)]
    source_post: Option<postkit_core::SourcePost>,
    scheduled_at: Option<DateTime<Utc>>,
}

#[utoipa::path(
    put,
    path = "/scheduled/{id}",
    params(("id" = i64, Path, description = "Post ID")),
    request_body = UpdateBody,
    responses(
        (status = 204, description = "Post updated"),
        (status = 404, description = "Post not found or not updatable"),
        (status = 422, description = "Nothing to update"),
        (status = 500, description = "Internal error"),
    ),
    security(("api_key" = [])),
    tag = "posts"
)]
async fn update_scheduled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBody>,
) -> Result<StatusCode, (StatusCode, String)> {
    if body.source_post.is_none() && body.scheduled_at.is_none() {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, "nothing to update".into()));
    }
    let source_json = body
        .source_post
        .as_ref()
        .map(|sp| serde_json::to_string(sp))
        .transpose()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let ok = state
        .store
        .update(id, source_json.as_deref(), body.scheduled_at)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if ok {
        if let Some(at) = body.scheduled_at {
            if let Err(e) = state.queue.push(id, at.timestamp()).await {
                tracing::warn!(id, "queue: {e}");
            }
        }
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("post {id} not found or not in pending state")))
    }
}

// ─── POST /scheduled/:id/retry ───────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/scheduled/{id}/retry",
    params(("id" = i64, Path, description = "Post ID")),
    responses(
        (status = 204, description = "Post queued for retry"),
        (status = 404, description = "Post not found or not in failed state"),
        (status = 500, description = "Internal error"),
    ),
    security(("api_key" = [])),
    tag = "posts"
)]
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
        if let Err(e) = state.queue.push(id, Utc::now().timestamp()).await {
            tracing::warn!(id, "queue: {e}");
        }
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("post {id} not found or not in failed state")))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use postkit_core::{
        AccountInfo, Capabilities, PreparedPost, Provider, ProviderKind, PublishResult, SourcePost,
    };
    use postkit_store::Store;
    use tower::ServiceExt;

    use super::*;

    struct MockProvider;

    #[async_trait]
    impl Provider for MockProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Bluesky }
        fn account_id(&self) -> &str { "test" }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                max_text_graphemes: 300,
                max_media: 4,
                supports_threads: false,
                supports_alt_text: true,
            }
        }
        async fn verify(&self) -> anyhow::Result<AccountInfo> { unimplemented!() }
        fn compose(&self, _: &SourcePost) -> anyhow::Result<PreparedPost> { unimplemented!() }
        async fn execute(&self, _: &PreparedPost) -> anyhow::Result<PublishResult> {
            unimplemented!()
        }
    }

    async fn mem_state(api_key: Option<&str>) -> Arc<AppState> {
        let store = Store::open(":memory:").await.unwrap();
        Arc::new(AppState {
            store,
            providers: Arc::new(HashMap::new()),
            api_key: api_key.map(str::to_string),
            queue: crate::queue::build(None).await,
        })
    }

    async fn mem_state_with_provider() -> Arc<AppState> {
        let store = Store::open(":memory:").await.unwrap();
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        providers.insert("test".to_string(), Arc::new(MockProvider));
        Arc::new(AppState {
            store,
            providers: Arc::new(providers),
            api_key: None,
            queue: crate::queue::build(None).await,
        })
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
                Request::get("/scheduled")
                    .header("X-Api-Key", "wrong")
                    .body(Body::empty())
                    .unwrap(),
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
            .oneshot(
                Request::delete("/scheduled/999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn retry_not_found() {
        let resp = router(mem_state(None).await)
            .oneshot(
                Request::post("/scheduled/999/retry")
                    .body(Body::empty())
                    .unwrap(),
            )
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
    async fn update_not_found() {
        let body = serde_json::json!({"scheduled_at": "2026-05-01T10:00:00Z"});
        let resp = router(mem_state(None).await)
            .oneshot(
                Request::put("/scheduled/999")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_empty_body_returns_unprocessable() {
        let resp = router(mem_state(None).await)
            .oneshot(
                Request::put("/scheduled/1")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn update_pending_post_succeeds() {
        let state = mem_state_with_provider().await;
        // create a post first
        let schedule_body = serde_json::json!({
            "account_id": "test",
            "scheduled_at": "2026-04-21T10:00:00Z",
            "source_post": {"text": "original", "media": [], "hashtags": []}
        });
        router(state.clone())
            .oneshot(
                Request::post("/schedule")
                    .header("content-type", "application/json")
                    .body(Body::from(schedule_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let update_body = serde_json::json!({"scheduled_at": "2026-05-01T12:00:00Z"});
        let resp = router(state)
            .oneshot(
                Request::put("/scheduled/1")
                    .header("content-type", "application/json")
                    .body(Body::from(update_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
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

    #[tokio::test]
    async fn schedule_without_date_creates_draft() {
        let body = serde_json::json!({
            "account_id": "test",
            "source_post": {"text": "draft", "media": [], "hashtags": []}
        });
        let state = mem_state_with_provider().await;
        let resp = router(state.clone())
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
        let id: i64 = json["id"].as_i64().unwrap();
        let post = state.store.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(post.status, "draft");
    }

    #[tokio::test]
    async fn update_draft_with_date_promotes_to_pending() {
        let state = mem_state_with_provider().await;
        // create draft
        let draft_body = serde_json::json!({
            "account_id": "test",
            "source_post": {"text": "draft", "media": [], "hashtags": []}
        });
        router(state.clone())
            .oneshot(
                Request::post("/schedule")
                    .header("content-type", "application/json")
                    .body(Body::from(draft_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // promote via PUT
        let update_body = serde_json::json!({"scheduled_at": "2026-06-01T10:00:00Z"});
        let resp = router(state.clone())
            .oneshot(
                Request::put("/scheduled/1")
                    .header("content-type", "application/json")
                    .body(Body::from(update_body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let post = state.store.get_by_id(1).await.unwrap().unwrap();
        assert_eq!(post.status, "pending");
    }
}
