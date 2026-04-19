use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
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
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/schedule", post(schedule_post))
        .route("/scheduled", get(list_scheduled))
        .route("/scheduled/{id}", get(get_scheduled).delete(cancel_scheduled))
        .with_state(state)
}

// ─── GET /health ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

// ─── POST /schedule ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ScheduleBody {
    pub account_id: String,
    pub source_post: postkit_core::SourcePost,
    pub scheduled_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ScheduleResponse {
    id: i64,
}

async fn schedule_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ScheduleBody>,
) -> Result<Json<ScheduleResponse>, (StatusCode, String)> {
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

    Ok(Json(ScheduleResponse { id }))
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
    let filters = ListFilters {
        account_id: q.account_id,
        provider: q.provider,
        status: q.status,
        from: q.from,
        to: q.to,
        limit: q.limit,
        offset: q.offset,
    };
    let posts = state
        .store
        .list(&filters)
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
    let cancelled = state
        .store
        .cancel(id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if cancelled {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!("post {id} no encontrado o no está en estado pending"),
        ))
    }
}
