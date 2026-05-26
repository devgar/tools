use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

pub type AnyQueue = Arc<dyn JobQueue>;

#[async_trait]
pub trait JobQueue: Send + Sync {
    async fn push(&self, post_id: i64, scheduled_at: i64) -> anyhow::Result<()>;
    async fn remove(&self, post_id: i64) -> anyhow::Result<()>;
    async fn pop_due(&self, limit: usize) -> anyhow::Result<Vec<i64>>;
    /// Seconds until the next queued job; None = queue empty.
    async fn next_due_in_secs(&self) -> anyhow::Result<Option<u64>>;
    /// Resolves when a new job is pushed (MemoryQueue) or after ~2 s (RedisQueue).
    async fn wait_for_activity(&self);
}

// ─── MemoryQueue ─────────────────────────────────────────────────────────────

struct MemInner {
    by_time: BTreeSet<(i64, i64)>, // (scheduled_at, post_id) — ordered for efficient min/range
    by_id: HashMap<i64, i64>,      // post_id → scheduled_at (for O(log n) removal)
}

pub struct MemoryQueue {
    inner: Mutex<MemInner>,
    notify: Notify,
}

impl MemoryQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MemInner { by_time: BTreeSet::new(), by_id: HashMap::new() }),
            notify: Notify::new(),
        }
    }
}

#[async_trait]
impl JobQueue for MemoryQueue {
    async fn push(&self, post_id: i64, scheduled_at: i64) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if let Some(old) = g.by_id.remove(&post_id) {
            g.by_time.remove(&(old, post_id));
        }
        g.by_time.insert((scheduled_at, post_id));
        g.by_id.insert(post_id, scheduled_at);
        drop(g);
        self.notify.notify_one();
        Ok(())
    }

    async fn remove(&self, post_id: i64) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if let Some(at) = g.by_id.remove(&post_id) {
            g.by_time.remove(&(at, post_id));
        }
        Ok(())
    }

    async fn pop_due(&self, limit: usize) -> anyhow::Result<Vec<i64>> {
        let now = Utc::now().timestamp();
        let mut g = self.inner.lock().await;
        // Range: all (at, id) where at <= now (i64::MAX covers any post_id)
        let due: Vec<(i64, i64)> = g
            .by_time
            .range(..=(now, i64::MAX))
            .take(limit)
            .cloned()
            .collect();
        for (at, id) in &due {
            g.by_time.remove(&(*at, *id));
            g.by_id.remove(id);
        }
        Ok(due.into_iter().map(|(_, id)| id).collect())
    }

    async fn next_due_in_secs(&self) -> anyhow::Result<Option<u64>> {
        let g = self.inner.lock().await;
        Ok(g.by_time.first().map(|(at, _)| {
            let now = Utc::now().timestamp();
            (*at - now).max(0) as u64
        }))
    }

    async fn wait_for_activity(&self) { self.notify.notified().await; }
}

// ─── RedisQueue ──────────────────────────────────────────────────────────────

const QUEUE_KEY: &str = "postkit:queue";

// Atomic: range by score ≤ now, then remove — all in one round-trip.
const POP_DUE_SCRIPT: &str = r#"
local due = redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', ARGV[1], 'LIMIT', 0, ARGV[2])
if #due > 0 then
    redis.call('ZREM', KEYS[1], unpack(due))
end
return due
"#;

pub struct RedisQueue {
    client: redis::Client,
}

impl RedisQueue {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let client =
            redis::Client::open(url).map_err(|e| anyhow::anyhow!("invalid Redis URL: {e}"))?;
        let mut conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!("Redis unreachable at {url}: {e}"))?;
        redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .map_err(|e| anyhow::anyhow!("Redis PING failed: {e}"))?;
        Ok(Self { client })
    }

    async fn conn(&self) -> anyhow::Result<redis::aio::MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!("Redis error: {e}"))
    }
}

#[async_trait]
impl JobQueue for RedisQueue {
    async fn push(&self, post_id: i64, scheduled_at: i64) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        redis::cmd("ZADD")
            .arg(QUEUE_KEY)
            .arg(scheduled_at)
            .arg(post_id.to_string())
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }

    async fn remove(&self, post_id: i64) -> anyhow::Result<()> {
        let mut conn = self.conn().await?;
        redis::cmd("ZREM")
            .arg(QUEUE_KEY)
            .arg(post_id.to_string())
            .query_async::<()>(&mut conn)
            .await?;
        Ok(())
    }

    async fn pop_due(&self, limit: usize) -> anyhow::Result<Vec<i64>> {
        let now = Utc::now().timestamp();
        let mut conn = self.conn().await?;
        let ids: Vec<String> = redis::cmd("EVAL")
            .arg(POP_DUE_SCRIPT)
            .arg(1)
            .arg(QUEUE_KEY)
            .arg(now)
            .arg(limit as i64)
            .query_async(&mut conn)
            .await?;
        Ok(ids.iter().filter_map(|s| s.parse::<i64>().ok()).collect())
    }

    async fn next_due_in_secs(&self) -> anyhow::Result<Option<u64>> {
        let mut conn = self.conn().await?;
        let result: Vec<(String, f64)> = redis::cmd("ZRANGEBYSCORE")
            .arg(QUEUE_KEY)
            .arg("-inf")
            .arg("+inf")
            .arg("WITHSCORES")
            .arg("LIMIT")
            .arg(0i64)
            .arg(1i64)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();
        Ok(result.first().map(|(_, score)| {
            let now = Utc::now().timestamp();
            (*score as i64 - now).max(0) as u64
        }))
    }

    async fn wait_for_activity(&self) {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

// ─── Factory ─────────────────────────────────────────────────────────────────

pub async fn build(redis_url: Option<&str>) -> AnyQueue {
    if let Some(url) = redis_url {
        match RedisQueue::connect(url).await {
            Ok(q) => {
                info!("queue: using Redis at {url}");
                return Arc::new(q);
            }
            Err(e) => {
                warn!("queue: Redis unavailable ({e}), falling back to in-memory queue");
            }
        }
    }
    info!("queue: using in-memory queue");
    Arc::new(MemoryQueue::new())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn past() -> i64 { Utc::now().timestamp() - 60 }
    fn future() -> i64 { Utc::now().timestamp() + 3600 }

    #[tokio::test]
    async fn push_and_pop_due_past() {
        let q = MemoryQueue::new();
        q.push(1, past()).await.unwrap();
        let ids = q.pop_due(10).await.unwrap();
        assert_eq!(ids, vec![1]);
    }

    #[tokio::test]
    async fn pop_due_skips_future() {
        let q = MemoryQueue::new();
        q.push(1, future()).await.unwrap();
        let ids = q.pop_due(10).await.unwrap();
        assert!(ids.is_empty());
    }

    #[tokio::test]
    async fn pop_due_removes_from_queue() {
        let q = MemoryQueue::new();
        q.push(1, past()).await.unwrap();
        q.pop_due(10).await.unwrap();
        // second pop should return nothing
        assert!(q.pop_due(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn remove_prevents_pop() {
        let q = MemoryQueue::new();
        q.push(1, past()).await.unwrap();
        q.remove(1).await.unwrap();
        assert!(q.pop_due(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn push_deduplicates_same_id() {
        let q = MemoryQueue::new();
        q.push(1, future()).await.unwrap();
        // re-push with a past timestamp — the entry should now be due
        q.push(1, past()).await.unwrap();
        let ids = q.pop_due(10).await.unwrap();
        assert_eq!(ids, vec![1]);
    }

    #[tokio::test]
    async fn pop_due_respects_limit() {
        let q = MemoryQueue::new();
        for id in 1i64..=5 {
            q.push(id, past() - id).await.unwrap(); // distinct timestamps
        }
        let ids = q.pop_due(3).await.unwrap();
        assert_eq!(ids.len(), 3);
        // remaining two are still in queue
        assert_eq!(q.pop_due(10).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn next_due_in_secs_empty_queue() {
        let q = MemoryQueue::new();
        assert!(q.next_due_in_secs().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn next_due_in_secs_future_item() {
        let q = MemoryQueue::new();
        q.push(1, Utc::now().timestamp() + 100).await.unwrap();
        let secs = q.next_due_in_secs().await.unwrap().unwrap();
        // allow a little clock drift in CI
        assert!(secs <= 100 && secs >= 95);
    }

    #[tokio::test]
    async fn next_due_in_secs_past_item_returns_zero() {
        let q = MemoryQueue::new();
        q.push(1, past()).await.unwrap();
        assert_eq!(q.next_due_in_secs().await.unwrap(), Some(0));
    }

    #[tokio::test]
    async fn wait_for_activity_resolves_after_push() {
        let q = Arc::new(MemoryQueue::new());
        let q2 = q.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            q2.push(1, past()).await.unwrap();
        });
        tokio::time::timeout(tokio::time::Duration::from_millis(500), q.wait_for_activity())
            .await
            .expect("wait_for_activity should resolve after push");
    }
}
