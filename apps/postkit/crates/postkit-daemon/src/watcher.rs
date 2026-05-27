use std::{collections::HashMap, sync::Arc};

use postkit_core::Provider;
use postkit_store::Store;
use tokio::{
    sync::{watch, RwLock},
    time::{sleep, Duration},
};
use tracing::{info, warn};

use crate::accounts::{build_providers, AccountsRepository, AppsRepository};

pub async fn run(
    apps_repo: Arc<dyn AppsRepository>,
    accounts_repo: Arc<dyn AccountsRepository>,
    store: Store,
    providers: Arc<RwLock<HashMap<String, Arc<dyn Provider>>>>,
    interval_secs: u64,
    mut shutdown: watch::Receiver<bool>,
) {
    info!("watcher started (interval={}s)", interval_secs);
    loop {
        tokio::select! {
            _ = sleep(Duration::from_secs(interval_secs)) => {}
            _ = shutdown.changed() => break,
        }

        if !apps_repo.has_changed().await && !accounts_repo.has_changed().await {
            continue;
        }

        let apps = match apps_repo.load().await {
            Ok(a) => a,
            Err(e) => {
                warn!("watcher: failed to load apps, keeping current providers: {e}");
                continue;
            }
        };
        let accounts = match accounts_repo.load().await {
            Ok(a) => a,
            Err(e) => {
                warn!("watcher: failed to load accounts, keeping current providers: {e}");
                continue;
            }
        };

        match build_providers(&apps, accounts, &store).await {
            Ok(new_map) => {
                let n = new_map.len();
                *providers.write().await = new_map;
                info!("watcher: providers reloaded ({n} accounts)");
            }
            Err(e) => warn!("watcher: provider rebuild failed, keeping current: {e}"),
        }
    }
    info!("watcher stopped");
}
