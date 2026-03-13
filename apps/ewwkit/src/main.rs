mod config;
mod domain;
mod infrastructure;
mod popup;
mod state;

use crate::config::AppConfig;
use crate::domain::SystemState;
use crate::popup::{PopupManager, PopupAction as InternalPopupAction};
use crate::infrastructure::ipc::{IpcServer, IpcMessage, PopupAction as IpcPopupAction, send_message};
use clap::{Parser, Subcommand};
use tokio::time::{interval, Duration};
use tokio::sync::mpsc;
use std::process::Command;

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
            handle_action(config, action).await?;
        }
    }

    Ok(())
}

async fn run_daemon(config: AppConfig) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<String>(32);
    let mut popup_manager = PopupManager::new(config.popups.timeout_ms, tx);
    let ipc_server = IpcServer::new(&config.ipc.socket_path)?;
    
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
                // Ejecutar comando real de EWW
                let args: Vec<&str> = cmd.split_whitespace().collect();
                if args.len() >= 2 {
                    let _ = Command::new("eww")
                        .args(["-c", "legacy", args[0], args[1]]) // Usamos legacy por ahora para pruebas
                        .spawn();
                }
            }
            msg = async { Some(ipc_server.accept_message()) } => {
                if let Some(Some(IpcMessage::Popup { name, action })) = msg {
                    let internal_action = match action {
                        IpcPopupAction::Open => InternalPopupAction::Open(name),
                        IpcPopupAction::Close => InternalPopupAction::Close(name),
                        IpcPopupAction::KeepAlive => InternalPopupAction::KeepAlive(name),
                    };
                    let _ = popup_manager.handle_action(internal_action).await;
                }
            }
        }
    }
}

async fn handle_action(config: AppConfig, action: ActionCommands) -> anyhow::Result<()> {
    let msg = match action {
        ActionCommands::Popup { name, close, keep_alive } => {
            let action = if close {
                IpcPopupAction::Close
            } else if keep_alive {
                IpcPopupAction::KeepAlive
            } else {
                IpcPopupAction::Open
            };
            IpcMessage::Popup { name, action }
        }
    };

    send_message(&config.ipc.socket_path, &msg)?;
    Ok(())
}
