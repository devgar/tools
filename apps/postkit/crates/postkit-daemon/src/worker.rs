use std::{collections::HashMap, sync::Arc};

use postkit_core::Provider;
use postkit_store::{ScheduledPost, Store};
use tokio::{
    sync::watch,
    time::{sleep, Duration},
};
use tracing::{error, info, warn};

use crate::queue::AnyQueue;

pub async fn run(
    store: Store,
    queue: AnyQueue,
    providers: Arc<HashMap<String, Arc<dyn Provider>>>,
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
                        let providers = providers.clone();
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
            Err(e) => error!("worker: error en pop_due: {e}"),
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
    providers: &HashMap<String, Arc<dyn Provider>>,
) -> anyhow::Result<Option<String>> {
    let provider = providers
        .get(&post.account_id)
        .ok_or_else(|| anyhow::anyhow!("unknown account: {}", post.account_id))?;
    let source: postkit_core::SourcePost = serde_json::from_str(&post.source_post)?;
    let resolved = source.resolve(provider.kind());
    let prepared = provider.compose(&resolved)?;
    let result = provider.execute(&prepared).await?;
    Ok(result.post_url)
}
