use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use async_trait::async_trait;
use postkit_core::{Provider, TokenSink};
use postkit_providers_bluesky::Bluesky;
use postkit_providers_meta::{FacebookPage, Instagram, MetaProvider};
use postkit_providers_x::X;
use postkit_store::Store;
use tracing::warn;

use crate::config::{load_accounts, AccountConfig, AppConfig};

// ─── Traits ──────────────────────────────────────────────────────────────────

#[async_trait]
pub trait AppsRepository: Send + Sync {
    async fn load(&self) -> anyhow::Result<HashMap<String, AppConfig>>;
    /// True if data may have changed since the last call (conservative is fine).
    async fn has_changed(&self) -> bool;
}

#[async_trait]
pub trait AccountsRepository: Send + Sync {
    async fn load(&self) -> anyhow::Result<HashMap<String, AccountConfig>>;
    async fn has_changed(&self) -> bool;
}

// ─── File-backed implementations ─────────────────────────────────────────────

fn file_mtime(path: &str) -> Option<SystemTime> { std::fs::metadata(path).ok()?.modified().ok() }

pub struct FileAppsRepository {
    path: String,
    last_mtime: Mutex<Option<SystemTime>>,
}

impl FileAppsRepository {
    pub fn new(path: String) -> Self { Self { path, last_mtime: Mutex::new(None) } }
}

#[async_trait]
impl AppsRepository for FileAppsRepository {
    async fn load(&self) -> anyhow::Result<HashMap<String, AppConfig>> {
        let (apps, _) = load_accounts(&self.path)?;
        Ok(apps)
    }

    async fn has_changed(&self) -> bool {
        let current = file_mtime(&self.path);
        let mut guard = self.last_mtime.lock().unwrap();
        if *guard == current {
            return false;
        }
        *guard = current;
        true
    }
}

pub struct FileAccountsRepository {
    path: String,
    last_mtime: Mutex<Option<SystemTime>>,
}

impl FileAccountsRepository {
    pub fn new(path: String) -> Self { Self { path, last_mtime: Mutex::new(None) } }
}

#[async_trait]
impl AccountsRepository for FileAccountsRepository {
    async fn load(&self) -> anyhow::Result<HashMap<String, AccountConfig>> {
        let (_, accounts) = load_accounts(&self.path)?;
        Ok(accounts)
    }

    async fn has_changed(&self) -> bool {
        let current = file_mtime(&self.path);
        let mut guard = self.last_mtime.lock().unwrap();
        if *guard == current {
            return false;
        }
        *guard = current;
        true
    }
}

// ─── Provider builder ─────────────────────────────────────────────────────────

/// If the token expires in less than 7 days it is considered stale.
const REFRESH_THRESHOLD_SECS: i64 = 7 * 24 * 3600;

async fn load_stored_token(store: &Store, account_id: &str) -> Option<(String, bool)> {
    let tokens = store.load_credential(account_id).await.ok()??;
    let now = chrono::Utc::now().timestamp();
    match tokens.expires_at {
        Some(exp) if exp <= now => None,
        Some(exp) => Some((tokens.access_token, exp > now + REFRESH_THRESHOLD_SECS)),
        None => Some((tokens.access_token, false)),
    }
}

async fn setup_meta_provider<P: MetaProvider>(
    mut provider: P,
    app: &str,
    fresh: bool,
    account_id: &str,
    apps: &HashMap<String, AppConfig>,
) -> Arc<dyn Provider> {
    let Some(AppConfig::Meta { app_id: Some(app_id), app_secret: Some(app_secret) }) =
        apps.get(app)
    else {
        return Arc::new(provider);
    };

    provider = provider.with_app_credentials(app_id.clone(), app_secret.clone());

    if fresh {
        return Arc::new(provider);
    }

    if let Err(e) = provider.ensure_fresh_token().await {
        warn!(account = account_id, "error refreshing Meta token on startup: {e}");
    }

    Arc::new(provider)
}

pub async fn build_providers(
    apps: &HashMap<String, AppConfig>,
    accounts: HashMap<String, AccountConfig>,
    store: &Store,
) -> anyhow::Result<HashMap<String, Arc<dyn Provider>>> {
    let sink: Arc<dyn TokenSink> = Arc::new(store.clone());
    let mut out: HashMap<String, Arc<dyn Provider>> = HashMap::new();

    for (id, acc) in accounts {
        match acc {
            AccountConfig::Bluesky { handle, app_password } => {
                let provider =
                    Bluesky::new(id.clone(), handle, app_password).with_token_sink(sink.clone());
                out.insert(id, Arc::new(provider));
            }

            AccountConfig::X { app, access_token, access_token_secret } => {
                let AppConfig::X { api_key, api_secret } = apps
                    .get(&app)
                    .ok_or_else(|| anyhow::anyhow!("app '{app}' not found"))?
                else {
                    anyhow::bail!("app '{app}' is not of type x");
                };
                out.insert(
                    id.clone(),
                    Arc::new(X::new(
                        id,
                        api_key.clone(),
                        api_secret.clone(),
                        access_token,
                        access_token_secret,
                    )),
                );
            }

            AccountConfig::FacebookPage { app, page_id, page_access_token } => {
                let (token, fresh) = load_stored_token(store, &id)
                    .await
                    .unwrap_or((page_access_token, false));
                let provider =
                    FacebookPage::new(id.clone(), page_id, token).with_token_sink(sink.clone());
                out.insert(id.clone(), setup_meta_provider(provider, &app, fresh, &id, apps).await);
            }

            AccountConfig::Instagram { app, ig_user_id, access_token } => {
                let (token, fresh) = load_stored_token(store, &id)
                    .await
                    .unwrap_or((access_token, false));
                let provider =
                    Instagram::new(id.clone(), ig_user_id, token).with_token_sink(sink.clone());
                out.insert(id.clone(), setup_meta_provider(provider, &app, fresh, &id, apps).await);
            }
        }
    }

    Ok(out)
}
