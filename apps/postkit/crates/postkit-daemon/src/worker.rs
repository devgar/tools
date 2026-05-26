use std::{collections::HashMap, sync::Arc};

use postkit_core::Provider;
use postkit_store::{ScheduledPost, Store};
use tokio::{
    sync::{watch, RwLock},
    time::{sleep, Duration},
};
use tracing::{error, info, warn};

use crate::queue::AnyQueue;

pub async fn run(
    store: Store,
    queue: AnyQueue,
    providers: Arc<RwLock<HashMap<String, Arc<dyn Provider>>>>,
    poll_secs: u64,
    max_attempts: u32,
    retry_delay_secs: u64,
    mut shutdown: watch::Receiver<bool>,
) {
    info!(
        "worker started (poll={}s, max_attempts={}, retry_delay={}s)",
        poll_secs, max_attempts, retry_delay_secs
    );
    loop {
        match queue.pop_due(10).await {
            Ok(ids) if !ids.is_empty() => match store.claim_by_ids(&ids).await {
                Ok(posts) if !posts.is_empty() => {
                    info!("worker: {} posts claimed", posts.len());
                    for post in posts {
                        let store = store.clone();
                        let providers = Arc::clone(&providers);
                        tokio::spawn(async move {
                            let id = post.id;
                            let account = post.account_id.clone();
                            match publish(&post, &providers).await {
                                Ok(url) => {
                                    info!(
                                        id,
                                        account,
                                        url = url.as_deref().unwrap_or("-"),
                                        "published"
                                    );
                                    let _ = store.mark_published(id, url.as_deref()).await;
                                }
                                Err(e) => {
                                    let attempt = post.attempts + 1;
                                    warn!(id, account, attempt, error = %e, "publish failed");
                                    let _ = store
                                        .attempt_or_fail(
                                            id,
                                            &e.to_string(),
                                            max_attempts,
                                            retry_delay_secs,
                                        )
                                        .await;
                                }
                            }
                        });
                    }
                }
                Ok(_) => {}
                Err(e) => error!("worker: failed to claim posts: {e}"),
            },
            Ok(_) => {}
            Err(e) => error!("worker: error in pop_due: {e}"),
        }

        let sleep_secs = match queue.next_due_in_secs().await {
            Ok(Some(secs)) => secs.min(poll_secs),
            Ok(None) | Err(_) => poll_secs,
        };

        tokio::select! {
            _ = sleep(Duration::from_secs(sleep_secs)) => {}
            _ = queue.wait_for_activity() => {}
            _ = shutdown.changed() => {
                info!("worker: shutdown signal received, stopping");
                break;
            }
        }
    }
    info!("worker stopped");
}

async fn publish(
    post: &ScheduledPost,
    providers: &RwLock<HashMap<String, Arc<dyn Provider>>>,
) -> anyhow::Result<Option<String>> {
    // Clone the Arc to release the read lock before any async work.
    let provider = providers
        .read()
        .await
        .get(&post.account_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown account: {}", post.account_id))?;
    let source: postkit_core::SourcePost = serde_json::from_str(&post.source_post)?;
    let resolved = source.resolve(provider.kind());
    let prepared = provider.compose(&resolved)?;
    let result = provider.execute(&prepared).await?;
    Ok(result.post_url)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use chrono::Utc;
    use postkit_core::{
        AccountInfo, Capabilities, PreparedPost, Provider, ProviderKind, PublishResult, SourcePost,
    };

    use super::*;

    fn make_post(account_id: &str, source_json: &str) -> ScheduledPost {
        ScheduledPost {
            id: 1,
            account_id: account_id.to_string(),
            provider: "bluesky".to_string(),
            source_post: source_json.to_string(),
            scheduled_at: Utc::now(),
            status: "running".to_string(),
            attempts: 0,
            published_at: None,
            post_url: None,
            error: None,
            created_at: Utc::now(),
        }
    }

    fn valid_source() -> &'static str {
        r#"{"text":"hello","media":[],"hashtags":[],"platforms":{}}"#
    }

    struct OkProvider {
        url: Option<String>,
    }

    #[async_trait]
    impl Provider for OkProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Bluesky }
        fn account_id(&self) -> &str { "acc" }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                max_text_graphemes: 300,
                max_media: 4,
                supports_threads: false,
                supports_alt_text: true,
            }
        }
        async fn verify(&self) -> anyhow::Result<AccountInfo> { unimplemented!() }
        fn compose(&self, _: &SourcePost) -> anyhow::Result<PreparedPost> {
            Ok(PreparedPost {
                account_id: "acc".into(),
                provider: ProviderKind::Bluesky,
                steps: vec![],
                warnings: vec![],
            })
        }
        async fn execute(&self, _: &PreparedPost) -> anyhow::Result<PublishResult> {
            Ok(PublishResult {
                platform_id: "123".into(),
                post_url: self.url.clone(),
                raw: serde_json::Value::Null,
            })
        }
    }

    struct FailProvider {
        on: &'static str,
    }

    #[async_trait]
    impl Provider for FailProvider {
        fn kind(&self) -> ProviderKind { ProviderKind::Bluesky }
        fn account_id(&self) -> &str { "acc" }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                max_text_graphemes: 300,
                max_media: 4,
                supports_threads: false,
                supports_alt_text: true,
            }
        }
        async fn verify(&self) -> anyhow::Result<AccountInfo> { unimplemented!() }
        fn compose(&self, _: &SourcePost) -> anyhow::Result<PreparedPost> {
            if self.on == "compose" {
                anyhow::bail!("compose failed");
            }
            Ok(PreparedPost {
                account_id: "acc".into(),
                provider: ProviderKind::Bluesky,
                steps: vec![],
                warnings: vec![],
            })
        }
        async fn execute(&self, _: &PreparedPost) -> anyhow::Result<PublishResult> {
            anyhow::bail!("execute failed")
        }
    }

    fn providers_with(
        id: &str,
        p: Arc<dyn Provider>,
    ) -> RwLock<HashMap<String, Arc<dyn Provider>>> {
        let mut m = HashMap::new();
        m.insert(id.to_string(), p);
        RwLock::new(m)
    }

    #[tokio::test]
    async fn publish_unknown_account_returns_error() {
        let post = make_post("ghost", valid_source());
        let result = publish(&post, &RwLock::new(HashMap::new())).await;
        assert!(result.unwrap_err().to_string().contains("unknown account"));
    }

    #[tokio::test]
    async fn publish_invalid_source_json_returns_error() {
        let post = make_post("acc", "not json");
        let providers = providers_with("acc", Arc::new(OkProvider { url: None }));
        assert!(publish(&post, &providers).await.is_err());
    }

    #[tokio::test]
    async fn publish_success_returns_url() {
        let post = make_post("acc", valid_source());
        let providers =
            providers_with("acc", Arc::new(OkProvider { url: Some("https://example.com".into()) }));
        let url = publish(&post, &providers).await.unwrap();
        assert_eq!(url, Some("https://example.com".to_string()));
    }

    #[tokio::test]
    async fn publish_success_no_url() {
        let post = make_post("acc", valid_source());
        let providers = providers_with("acc", Arc::new(OkProvider { url: None }));
        assert_eq!(publish(&post, &providers).await.unwrap(), None);
    }

    #[tokio::test]
    async fn publish_compose_failure_returns_error() {
        let post = make_post("acc", valid_source());
        let providers = providers_with("acc", Arc::new(FailProvider { on: "compose" }));
        let err = publish(&post, &providers).await.unwrap_err();
        assert!(err.to_string().contains("compose failed"));
    }

    #[tokio::test]
    async fn publish_execute_failure_returns_error() {
        let post = make_post("acc", valid_source());
        let providers = providers_with("acc", Arc::new(FailProvider { on: "execute" }));
        let err = publish(&post, &providers).await.unwrap_err();
        assert!(err.to_string().contains("execute failed"));
    }
}
