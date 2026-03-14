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
    /// Lista todas las ventanas
    Windows,
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
        Commands::Windows => {
            list_windows(config).await?;
        }
    }

    Ok(())
}

async fn run_daemon(config: AppConfig) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<String>(32);
    let mut popup_manager = PopupManager::new(config.popups.timeout_ms, tx);
    let ipc_server = IpcServer::new(&config.ipc.socket_path)?;
    let sys_adapter = SysfsAdapter::new("BAT0");
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path);
    
    let mut state = SystemState::default();
    let mut last_emitted_state = SystemState::default();
    
    let mut poll_interval = interval(Duration::from_millis(config.polling.battery_ms));
    let mut popup_check = interval(Duration::from_millis(100));
    let niri_socket_path = config.niri.socket_path.clone().unwrap_or_else(|| {
        std::env::var("NIRI_SOCKET").unwrap_or_else(|_| {
            let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
            format!("{}/niri-0", xdg_runtime)
        })
    });
    let mut niri_events = NiriAdapter::event_listener(niri_socket_path).await?;

    loop {
        tokio::select! {
            _ = poll_interval.tick() => {
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
                if let Ok(workspaces) = niri_adapter.get_workspaces().await {
                    state.desktop.workspaces = workspaces;
                }
                if let Ok(windows) = niri_adapter.get_windows().await {
                    state.desktop.windows = windows;
                }
                if let Ok(focused_id) = niri_adapter.get_focused_window_id().await {
                    state.desktop.focused_window_id = focused_id;
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

        // Deduplicación en la capa de aplicación
        if state_changed && state != last_emitted_state {
            let _ = presenter.update_state(&state).await;
            last_emitted_state = state.clone();
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

async fn list_windows(config: AppConfig) -> anyhow::Result<()> {
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path);
    let windows = niri_adapter.get_windows().await?;
    for win in windows {
        println!("Window ID: {}, Title: {}, App ID: {:?}, Workspace ID: {}, Focused: {}", 
            win.id, win.title, win.app_id, win.workspace_id, win.is_focused);
    }
    Ok(())
}
