use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// Fila pública expuesta por la store. Los timestamps se exponen como DateTime<Utc>.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledPost {
    pub id: i64,
    pub account_id: String,
    pub provider: String,
    pub source_post: String,
    pub scheduled_at: DateTime<Utc>,
    pub status: String,
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
        let url = format!("sqlite:{path}?mode=rwc");
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
            "SELECT id, account_id, provider, source_post, scheduled_at, status, \
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
             RETURNING id, account_id, provider, source_post, scheduled_at, status, \
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
