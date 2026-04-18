mod config;
mod routes;
mod worker;

use anyhow::Result;
use clap::Parser;
use postkit_core::Provider;
use postkit_providers_bluesky::Bluesky;
use postkit_providers_x::X;
use postkit_store::Store;
use routes::AppState;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use config::{AccountConfig, DaemonConfig};

#[derive(Parser)]
#[command(name = "postkit-daemon")]
struct Cli {
    #[arg(long, default_value = "daemon.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = DaemonConfig::load(&cli.config)?;

    let store = Store::open(&cfg.db_path).await?;
    let accounts = config::load_accounts(&cfg.accounts_config)?;
    let providers = Arc::new(build_providers(accounts));

    let state = Arc::new(AppState {
        store: store.clone(),
        providers: providers.clone(),
    });

    let addr: SocketAddr = cfg.listen.parse()?;
    let app = routes::router(state);

    tokio::spawn(worker::run(store, providers, cfg.poll_interval_secs));

    println!("postkit-daemon escuchando en {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_providers(accounts: HashMap<String, AccountConfig>) -> HashMap<String, Arc<dyn Provider>> {
    let mut out: HashMap<String, Arc<dyn Provider>> = HashMap::new();
    for (id, acc) in accounts {
        match acc {
            AccountConfig::Bluesky { handle, app_password } => {
                out.insert(
                    id.clone(),
                    Arc::new(Bluesky::new(id, handle, app_password)),
                );
            }
            AccountConfig::X { api_key, api_secret, access_token, access_token_secret } => {
                out.insert(
                    id.clone(),
                    Arc::new(X::new(id, api_key, api_secret, access_token, access_token_secret)),
                );
            }
        }
    }
    out
}
