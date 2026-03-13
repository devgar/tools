mod config;
mod domain;
mod infrastructure;
mod popup;
mod state;

use crate::config::AppConfig;
use crate::domain::SystemState;
use crate::popup::{PopupManager};
use clap::{Parser, Subcommand};
use tokio::time::{interval, Duration};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inicia el daemon de monitorización
    Daemon,
    /// Ejecuta una acción (ej. abrir popup)
    Action {
        #[command(subcommand)]
        action: ActionCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ActionCommands {
    Popup {
        name: String,
        #[arg(short, long)]
        close: bool,
        #[arg(short, long, name = "keep_alive_flag")]
        keep_alive: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::new()?;

    match cli.command {
        Commands::Daemon => {
            run_daemon(config).await?;
        }
        Commands::Action { action } => {
            handle_action(action).await?;
        }
    }

    Ok(())
}

async fn run_daemon(config: AppConfig) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<String>(32);
    let mut popup_manager = PopupManager::new(config.popups.timeout_ms, tx);
    
    let state = SystemState::default();
    let mut poll_interval = interval(Duration::from_millis(config.polling.battery_ms));
    let mut popup_check = interval(Duration::from_millis(100));

    loop {
        tokio::select! {
            _ = poll_interval.tick() => {
                let json = serde_json::to_string(&state)?;
                println!("{}", json);
            }
            _ = popup_check.tick() => {
                let _ = popup_manager.check_timeouts().await;
            }
            Some(cmd) = rx.recv() => {
                eprintln!("EWW Command: {}", cmd);
            }
        }
    }
}

async fn handle_action(action: ActionCommands) -> anyhow::Result<()> {
    println!("Action: {:?}", action);
    Ok(())
}
