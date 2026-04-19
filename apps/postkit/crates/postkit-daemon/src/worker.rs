use postkit_core::Provider;
use postkit_store::{ScheduledPost, Store};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

pub async fn run(
    store: Store,
    providers: Arc<HashMap<String, Arc<dyn Provider>>>,
    poll_secs: u64,
    max_attempts: u32,
    retry_delay_secs: u64,
) {
    info!("worker iniciado (poll={}s, max_attempts={}, retry_delay={}s)", poll_secs, max_attempts, retry_delay_secs);
    loop {
        match store.claim_due().await {
            Ok(due) if !due.is_empty() => {
                info!("worker: {} posts reclamados", due.len());
                for post in due {
                    let store = store.clone();
                    let providers = providers.clone();
                    tokio::spawn(async move {
                        let id = post.id;
                        let account = post.account_id.clone();
                        match publish(&post, &providers).await {
                            Ok(url) => {
                                info!(id, account, url = url.as_deref().unwrap_or("-"), "publicado");
                                let _ = store.mark_published(id, url.as_deref()).await;
                            }
                            Err(e) => {
                                let attempt = post.attempts + 1;
                                warn!(id, account, attempt, error = %e, "fallo en publicación");
                                let _ = store
                                    .attempt_or_fail(id, &e.to_string(), max_attempts, retry_delay_secs)
                                    .await;
                            }
                        }
                    });
                }
            }
            Ok(_) => {}
            Err(e) => error!("worker: error reclamando posts: {}", e),
        }
        sleep(Duration::from_secs(poll_secs)).await;
    }
}

async fn publish(
    post: &ScheduledPost,
    providers: &HashMap<String, Arc<dyn Provider>>,
) -> anyhow::Result<Option<String>> {
    let provider = providers
        .get(&post.account_id)
        .ok_or_else(|| anyhow::anyhow!("cuenta desconocida: {}", post.account_id))?;
    let source: postkit_core::SourcePost = serde_json::from_str(&post.source_post)?;
    let prepared = provider.compose(&source)?;
    let result = provider.execute(&prepared).await?;
    Ok(result.post_url)
}
