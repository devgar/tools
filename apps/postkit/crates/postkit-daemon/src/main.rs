mod config;
mod queue;
mod routes;
mod worker;

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Result;
use clap::Parser;
use config::{AccountConfig, AppConfig, DaemonConfig};
use postkit_core::{Provider, TokenSink};
use postkit_providers_bluesky::Bluesky;
use postkit_providers_meta::{FacebookPage, Instagram, MetaProvider};
use postkit_providers_x::X;
use postkit_store::{ListFilters, Store};
use routes::AppState;
use tokio::sync::watch;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "postkit-daemon")]
struct Cli {
    /// Ruta al archivo de configuración. Por defecto: ~/.config/postkit/daemon.toml
    #[arg(long)]
    config: Option<PathBuf>,
}

fn find_config() -> Option<PathBuf> {
    // 1. $XDG_CONFIG_HOME/postkit/daemon.toml  (~/.config/postkit/daemon.toml)
    let user_base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(p) = user_base.map(|b| b.join("postkit/daemon.toml")) {
        if p.exists() {
            return Some(p);
        }
    }
    // 2. $XDG_CONFIG_DIRS/postkit/daemon.toml  (default: /etc/xdg/postkit/daemon.toml)
    let dirs =
        std::env::var_os("XDG_CONFIG_DIRS").unwrap_or_else(|| std::ffi::OsString::from("/etc/xdg"));
    for dir in std::env::split_paths(&dirs) {
        let p = dir.join("postkit/daemon.toml");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "postkit_daemon=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();
    let config_path = cli.config.or_else(find_config).ok_or_else(|| {
        anyhow::anyhow!(
            "daemon.toml not found; place it at $XDG_CONFIG_HOME/postkit/daemon.toml \
             (default: ~/.config/postkit/daemon.toml) or pass --config <path>"
        )
    })?;
    let cfg = DaemonConfig::load(&config_path)?;

    let store = Store::open(&cfg.db_path).await?;
    let (apps, accounts) = config::load_accounts(&cfg.accounts_config)?;
    let providers = Arc::new(build_providers(&apps, accounts, &store).await?);

    let queue = queue::build(cfg.redis_url.as_deref()).await;

    // Sync pending posts into the queue so nothing is missed on restart.
    let pending = store
        .list(&ListFilters { status: Some("pending".into()), ..Default::default() })
        .await?;
    info!("sync: {} posts pendientes cargados en la cola", pending.len());
    for post in &pending {
        if let Err(e) = queue.push(post.id, post.scheduled_at.timestamp()).await {
            warn!(id = post.id, "sync: error al encolar post: {e}");
        }
    }

    let state = Arc::new(AppState {
        store: store.clone(),
        providers: providers.clone(),
        api_key: cfg.api_key,
        queue: queue.clone(),
    });

    let addr: SocketAddr = cfg.listen.parse()?;
    let app = routes::router(state);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(worker::run(
        store,
        queue,
        providers,
        cfg.poll_interval_secs,
        cfg.max_attempts,
        cfg.retry_delay_secs,
        shutdown_rx,
    ));

    info!("postkit-daemon escuchando en {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    let _ = shutdown_tx.send(true);
    info!("daemon detenido");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let sigterm = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = sigterm => {}
    }
    info!("señal de cierre recibida");
}

/// Margen de renovación: si el token expira en menos de 7 días, se considera "a punto de caducar".
const REFRESH_THRESHOLD_SECS: i64 = 7 * 24 * 3600;

/// Devuelve `(token, fresco)` si existe en store y no ha expirado.
/// `fresco = true` significa que tiene más de 7 días de validez restante → no hace falta renovar.
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
        warn!(account = account_id, "error renovando token Meta en arranque: {e}");
    }

    Arc::new(provider)
}

async fn build_providers(
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
                    .ok_or_else(|| anyhow::anyhow!("app '{app}' no encontrada"))?
                else {
                    anyhow::bail!("app '{app}' no es de tipo x");
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
