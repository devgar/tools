use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row as _, SqlitePool};

/// Fila pública expuesta por la store. Los timestamps se exponen como DateTime<Utc>.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledPost {
    pub id: i64,
    pub account_id: String,
    pub provider: String,
    pub source_post: String,
    pub scheduled_at: DateTime<Utc>,
    pub status: String,
    pub attempts: i64,
    pub published_at: Option<DateTime<Utc>>,
    pub post_url: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Fila interna con timestamps como i64 (Unix epoch).
#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    account_id: String,
    provider: String,
    source_post: String,
    scheduled_at: i64,
    status: String,
    attempts: i64,
    published_at: Option<i64>,
    post_url: Option<String>,
    error: Option<String>,
    created_at: i64,
}

impl From<Row> for ScheduledPost {
    fn from(r: Row) -> Self {
        Self {
            id: r.id,
            account_id: r.account_id,
            provider: r.provider,
            source_post: r.source_post,
            scheduled_at: Utc.timestamp_opt(r.scheduled_at, 0).single().unwrap_or_default(),
            status: r.status,
            attempts: r.attempts,
            published_at: r.published_at.and_then(|t| Utc.timestamp_opt(t, 0).single()),
            post_url: r.post_url,
            error: r.error,
            created_at: Utc.timestamp_opt(r.created_at, 0).single().unwrap_or_default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ListFilters {
    pub account_id: Option<String>,
    pub provider: Option<String>,
    pub status: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &str) -> anyhow::Result<Self> {
        let url = if path == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite:{path}?mode=rwc")
        };
        let pool = SqlitePool::connect(&url).await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn schedule(
        &self,
        account_id: &str,
        provider: &str,
        source_post: &str,
        scheduled_at: DateTime<Utc>,
    ) -> anyhow::Result<i64> {
        let ts = scheduled_at.timestamp();
        let id = sqlx::query(
            "INSERT INTO scheduled_posts (account_id, provider, source_post, scheduled_at) VALUES (?, ?, ?, ?)",
        )
        .bind(account_id)
        .bind(provider)
        .bind(source_post)
        .bind(ts)
        .execute(&self.pool)
        .await?
        .last_insert_rowid();
        Ok(id)
    }

    pub async fn list(&self, f: &ListFilters) -> anyhow::Result<Vec<ScheduledPost>> {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id, account_id, provider, source_post, scheduled_at, status, attempts, \
             published_at, post_url, error, created_at FROM scheduled_posts WHERE 1=1",
        );
        if let Some(ref v) = f.account_id {
            qb.push(" AND account_id = ").push_bind(v.clone());
        }
        if let Some(ref v) = f.provider {
            qb.push(" AND provider = ").push_bind(v.clone());
        }
        if let Some(ref v) = f.status {
            qb.push(" AND status = ").push_bind(v.clone());
        }
        if let Some(from) = f.from {
            qb.push(" AND scheduled_at >= ").push_bind(from.timestamp());
        }
        if let Some(to) = f.to {
            qb.push(" AND scheduled_at <= ").push_bind(to.timestamp());
        }
        qb.push(" ORDER BY scheduled_at ASC");
        if let Some(limit) = f.limit {
            qb.push(" LIMIT ").push_bind(limit);
        }
        if let Some(offset) = f.offset {
            qb.push(" OFFSET ").push_bind(offset);
        }

        let rows: Vec<Row> = qb
            .build_query_as::<Row>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Reclama atómicamente los posts pendientes con scheduled_at <= ahora.
    /// Los marca como 'running' y los devuelve para ejecutar.
    pub async fn claim_due(&self) -> anyhow::Result<Vec<ScheduledPost>> {
        let rows: Vec<Row> = sqlx::query_as::<_, Row>(
            "UPDATE scheduled_posts SET status = 'running' \
             WHERE id IN ( \
                 SELECT id FROM scheduled_posts \
                 WHERE status = 'pending' AND scheduled_at <= unixepoch() \
                 LIMIT 10 \
             ) \
             RETURNING id, account_id, provider, source_post, scheduled_at, status, attempts, \
                       published_at, post_url, error, created_at",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn mark_published(&self, id: i64, url: Option<&str>) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE scheduled_posts SET status='published', published_at=unixepoch(), post_url=? WHERE id=?",
        )
        .bind(url)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: i64, error: &str) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE scheduled_posts SET status='failed', error=? WHERE id=?",
        )
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_id(&self, id: i64) -> anyhow::Result<Option<ScheduledPost>> {
        let row = sqlx::query_as::<_, Row>(
            "SELECT id, account_id, provider, source_post, scheduled_at, status, attempts, \
             published_at, post_url, error, created_at FROM scheduled_posts WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    /// Si quedan intentos disponibles, reprograma el post con backoff exponencial.
    /// Si se agotaron los intentos, lo marca como 'failed'.
    pub async fn attempt_or_fail(
        &self,
        id: i64,
        error: &str,
        max_attempts: u32,
        base_delay_secs: u64,
    ) -> anyhow::Result<()> {
        let row = sqlx::query("SELECT attempts FROM scheduled_posts WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        let attempts: i64 = row.get(0);
        let new_attempts = attempts + 1;

        if new_attempts >= max_attempts as i64 {
            sqlx::query(
                "UPDATE scheduled_posts SET status='failed', error=?, attempts=? WHERE id=?",
            )
            .bind(error)
            .bind(new_attempts)
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else {
            // backoff exponencial: delay * 2^attempts
            let delay = base_delay_secs.saturating_mul(1u64 << attempts);
            let retry_at = Utc::now().timestamp() + delay as i64;
            sqlx::query(
                "UPDATE scheduled_posts SET status='pending', error=?, attempts=?, scheduled_at=? WHERE id=?",
            )
            .bind(error)
            .bind(new_attempts)
            .bind(retry_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Resetea un post 'failed' a 'pending' con attempts=0 y scheduled_at=ahora.
    /// Devuelve true si se reintentó, false si el post no existe o no está en failed.
    pub async fn retry(&self, id: i64) -> anyhow::Result<bool> {
        let n = sqlx::query(
            "UPDATE scheduled_posts \
             SET status='pending', attempts=0, scheduled_at=unixepoch(), error=NULL \
             WHERE id=? AND status='failed'",
        )
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n > 0)
    }

    /// Cancela un post si está en estado 'pending'. Devuelve true si se canceló.
    pub async fn cancel(&self, id: i64) -> anyhow::Result<bool> {
        let n = sqlx::query(
            "UPDATE scheduled_posts SET status='cancelled' WHERE id=? AND status='pending'",
        )
        .bind(id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    async fn mem_store() -> Store {
        Store::open(":memory:").await.unwrap()
    }

    const SRC: &str = r#"{"text":"hola","media":[],"hashtags":[]}"#;

    #[tokio::test]
    async fn schedule_creates_pending_record() {
        let s = mem_store().await;
        let id = s.schedule("personal", "bluesky", SRC, Utc::now()).await.unwrap();
        assert_eq!(id, 1);

        let all = s.list(&ListFilters::default()).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, "pending");
        assert_eq!(all[0].account_id, "personal");
    }

    #[tokio::test]
    async fn list_filter_by_status() {
        let s = mem_store().await;
        let id1 = s.schedule("a", "bluesky", SRC, Utc::now()).await.unwrap();
        let _id2 = s.schedule("b", "x", SRC, Utc::now()).await.unwrap();
        s.mark_failed(id1, "boom").await.unwrap();

        let pending = s
            .list(&ListFilters { status: Some("pending".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].account_id, "b");

        let failed = s
            .list(&ListFilters { status: Some("failed".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn list_filter_by_provider_and_account() {
        let s = mem_store().await;
        s.schedule("alice", "bluesky", SRC, Utc::now()).await.unwrap();
        s.schedule("bob", "x", SRC, Utc::now()).await.unwrap();

        let bsky = s
            .list(&ListFilters { provider: Some("bluesky".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(bsky.len(), 1);
        assert_eq!(bsky[0].account_id, "alice");

        let bob = s
            .list(&ListFilters { account_id: Some("bob".into()), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(bob.len(), 1);
        assert_eq!(bob[0].provider, "x");
    }

    #[tokio::test]
    async fn list_filter_by_date_range() {
        let s = mem_store().await;
        let past = Utc::now() - Duration::hours(2);
        let future = Utc::now() + Duration::hours(2);
        s.schedule("a", "bluesky", SRC, past).await.unwrap();
        s.schedule("b", "bluesky", SRC, future).await.unwrap();

        let only_past = s
            .list(&ListFilters {
                to: Some(Utc::now()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(only_past.len(), 1);
        assert_eq!(only_past[0].account_id, "a");
    }

    #[tokio::test]
    async fn list_pagination() {
        let s = mem_store().await;
        for i in 0..5 {
            s.schedule(&format!("acc{i}"), "bluesky", SRC, Utc::now()).await.unwrap();
        }
        let page = s
            .list(&ListFilters { limit: Some(2), offset: Some(1), ..Default::default() })
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].account_id, "acc1");
    }

    #[tokio::test]
    async fn claim_due_returns_past_pending() {
        let s = mem_store().await;
        let past = Utc::now() - Duration::seconds(10);
        let future = Utc::now() + Duration::hours(1);
        s.schedule("due", "bluesky", SRC, past).await.unwrap();
        s.schedule("not_due", "bluesky", SRC, future).await.unwrap();

        let claimed = s.claim_due().await.unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].account_id, "due");
        assert_eq!(claimed[0].status, "running");

        // El mismo post no debe reclamarse dos veces
        let again = s.claim_due().await.unwrap();
        assert!(again.is_empty());
    }

    #[tokio::test]
    async fn mark_published_updates_fields() {
        let s = mem_store().await;
        let id = s.schedule("a", "bluesky", SRC, Utc::now()).await.unwrap();
        s.mark_published(id, Some("https://bsky.app/profile/a/post/123")).await.unwrap();

        let post = &s.list(&ListFilters::default()).await.unwrap()[0];
        assert_eq!(post.status, "published");
        assert_eq!(post.post_url.as_deref(), Some("https://bsky.app/profile/a/post/123"));
        assert!(post.published_at.is_some());
    }

    #[tokio::test]
    async fn cancel_pending_returns_true() {
        let s = mem_store().await;
        let id = s.schedule("a", "bluesky", SRC, Utc::now()).await.unwrap();
        assert!(s.cancel(id).await.unwrap());
        assert!(!s.cancel(id).await.unwrap()); // ya cancelado
    }

    #[tokio::test]
    async fn cancel_non_pending_returns_false() {
        let s = mem_store().await;
        let id = s.schedule("a", "bluesky", SRC, Utc::now()).await.unwrap();
        s.mark_published(id, None).await.unwrap();
        assert!(!s.cancel(id).await.unwrap());
    }

    #[tokio::test]
    async fn attempt_or_fail_reschedules_when_under_max() {
        let s = mem_store().await;
        let past = Utc::now() - Duration::seconds(10);
        let id = s.schedule("a", "bluesky", SRC, past).await.unwrap();
        s.claim_due().await.unwrap(); // status → running

        s.attempt_or_fail(id, "transient error", 3, 60).await.unwrap();

        let post = s.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(post.status, "pending");
        assert_eq!(post.attempts, 1);
        assert!(post.scheduled_at > Utc::now()); // rescheduled in future
        assert_eq!(post.error.as_deref(), Some("transient error"));
    }

    #[tokio::test]
    async fn attempt_or_fail_marks_failed_after_max_attempts() {
        let s = mem_store().await;
        let past = Utc::now() - Duration::seconds(10);
        let id = s.schedule("a", "bluesky", SRC, past).await.unwrap();
        s.claim_due().await.unwrap();

        s.attempt_or_fail(id, "boom", 1, 60).await.unwrap();

        let post = s.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(post.status, "failed");
        assert_eq!(post.attempts, 1);
        assert_eq!(post.error.as_deref(), Some("boom"));
    }

    #[tokio::test]
    async fn retry_resets_failed_to_pending() {
        let s = mem_store().await;
        let id = s.schedule("a", "bluesky", SRC, Utc::now()).await.unwrap();
        s.mark_failed(id, "old error").await.unwrap();

        assert!(s.retry(id).await.unwrap());

        let post = s.get_by_id(id).await.unwrap().unwrap();
        assert_eq!(post.status, "pending");
        assert_eq!(post.attempts, 0);
        assert!(post.error.is_none());
    }

    #[tokio::test]
    async fn retry_returns_false_for_non_failed() {
        let s = mem_store().await;
        let id = s.schedule("a", "bluesky", SRC, Utc::now()).await.unwrap();
        // still pending — retry should be a no-op
        assert!(!s.retry(id).await.unwrap());
        assert_eq!(s.get_by_id(id).await.unwrap().unwrap().status, "pending");
    }
}
