mod config;
mod domain;
mod infrastructure;
mod popup;
mod state;

use crate::config::AppConfig;
use crate::domain::{SystemState, SystemProvider, WindowManager};
use crate::popup::{PopupManager, PopupAction as InternalPopupAction};
use crate::infrastructure::ipc::{IpcServer, IpcMessage, PopupAction as IpcPopupAction, send_message};
use crate::infrastructure::sysfs::SysfsAdapter;
use crate::infrastructure::niri::NiriAdapter;
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
    let sys_adapter = SysfsAdapter::new("BAT0");
    let niri_adapter = NiriAdapter::new();
    
    let mut state = SystemState::default();
    let mut poll_interval = interval(Duration::from_millis(config.polling.battery_ms));
    let mut popup_check = interval(Duration::from_millis(100));
    let mut niri_events = NiriAdapter::event_listener().await?;

    loop {
        tokio::select! {
            _ = poll_interval.tick() => {
                // Polling para sensores del sistema
                if let Ok(bat) = sys_adapter.get_battery().await {
                    state.battery = bat;
                }
                if let Ok(net) = sys_adapter.get_network().await {
                    state.network = net;
                }
                if let Ok(audio) = sys_adapter.get_audio().await {
                    state.audio = audio;
                }

                let json = serde_json::to_string(&state)?;
                println!("{}", json);
            }
            Some(_) = niri_events.recv() => {
                // Reactivo para eventos de Niri
                if let Ok(workspaces) = niri_adapter.get_workspaces().await {
                    state.desktop.workspaces = workspaces;
                }
                if let Ok(focused) = niri_adapter.get_focused_window().await {
                    state.desktop.focused_window = focused;
                }

                let json = serde_json::to_string(&state)?;
                println!("{}", json);
            }
            _ = popup_check.tick() => {
                let _ = popup_manager.check_timeouts().await;
            }
            Some(cmd) = rx.recv() => {
                let args: Vec<&str> = cmd.split_whitespace().collect();
                if args.len() >= 2 {
                    let _ = Command::new("eww")
                        .args(["-c", "legacy", args[0], args[1]])
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
