use postkit_core::Provider;
use postkit_store::{ScheduledPost, Store};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

pub async fn run(
    store: Store,
    providers: Arc<HashMap<String, Arc<dyn Provider>>>,
    poll_secs: u64,
) {
    loop {
        match store.claim_due().await {
            Ok(due) => {
                for post in due {
                    let store = store.clone();
                    let providers = providers.clone();
                    tokio::spawn(async move {
                        let id = post.id;
                        match publish(&post, &providers).await {
                            Ok(url) => {
                                let _ = store.mark_published(id, url.as_deref()).await;
                                println!("✓ published post {id} → {}", url.as_deref().unwrap_or("-"));
                            }
                            Err(e) => {
                                let _ = store.mark_failed(id, &e.to_string()).await;
                                eprintln!("✗ post {id} failed: {e}");
                            }
                        }
                    });
                }
            }
            Err(e) => eprintln!("worker: error claiming due posts: {e}"),
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
