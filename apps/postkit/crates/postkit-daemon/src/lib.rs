pub mod accounts;
pub mod config;
pub mod queue;
pub mod routes;
pub mod watcher;
pub mod worker;

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};

use accounts::{
    build_providers, AccountsRepository, AppsRepository, FileAccountsRepository, FileAppsRepository,
};
use anyhow::Result;
use clap::Parser;
use config::{DaemonConfig, SourceConfig};
use postkit_core::Provider;
use postkit_store::{ListFilters, Store};
pub use routes::ApiDoc;
use routes::AppState;
use tokio::sync::{watch, RwLock};
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "postkit-daemon")]
struct Cli {
    /// Path to the configuration file. Default: ~/.config/postkit/daemon.toml
    #[arg(long)]
    config: Option<PathBuf>,
}

pub fn find_config() -> Option<PathBuf> {
    let user_base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    if let Some(p) = user_base.map(|b| b.join("postkit/daemon.toml")) {
        if p.exists() {
            return Some(p);
        }
    }
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

pub async fn run() -> Result<()> {
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

    let apps_repo = Arc::new(make_apps_repo(&cfg.apps));
    let accounts_repo = Arc::new(make_accounts_repo(&cfg.accounts));

    let apps = apps_repo.load().await?;
    let accounts = accounts_repo.load().await?;
    let providers: Arc<RwLock<HashMap<String, Arc<dyn Provider>>>> =
        Arc::new(RwLock::new(build_providers(&apps, accounts, &store).await?));

    let queue = queue::build(cfg.redis_url.as_deref()).await;

    let pending = store
        .list(&ListFilters { status: Some("pending".into()), ..Default::default() })
        .await?;
    info!("sync: {} pending posts loaded into queue", pending.len());
    for post in &pending {
        if let Err(e) = queue.push(post.id, post.scheduled_at.timestamp()).await {
            warn!(id = post.id, "sync: error queuing post: {e}");
        }
    }

    let state = Arc::new(AppState {
        store: store.clone(),
        providers: Arc::clone(&providers),
        api_key: cfg.api_key,
        queue: queue.clone(),
    });

    let addr: SocketAddr = cfg.listen.parse()?;
    let app = routes::router(state);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(worker::run(
        store.clone(),
        queue,
        Arc::clone(&providers),
        cfg.poll_interval_secs,
        cfg.max_attempts,
        cfg.retry_delay_secs,
        shutdown_rx.clone(),
    ));

    tokio::spawn(watcher::run(
        apps_repo,
        accounts_repo,
        store,
        Arc::clone(&providers),
        cfg.reload_interval_secs,
        shutdown_rx,
    ));

    info!("postkit-daemon listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    let _ = shutdown_tx.send(true);
    info!("daemon stopped");
    Ok(())
}

fn make_apps_repo(cfg: &SourceConfig) -> impl AppsRepository {
    match cfg {
        SourceConfig::File { path } => FileAppsRepository::new(path.clone()),
    }
}

fn make_accounts_repo(cfg: &SourceConfig) -> impl AccountsRepository {
    match cfg {
        SourceConfig::File { path } => FileAccountsRepository::new(path.clone()),
    }
}

pub async fn shutdown_signal() {
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
    info!("shutdown signal received");
}
