mod config;
mod domain;
mod infrastructure;
mod popup;
mod state;

use crate::config::AppConfig;
use crate::domain::{AppState, PopupState, Presenter, SystemProvider, WindowManager};
use crate::popup::{PopupManager, PopupAction as InternalPopupAction};
use crate::infrastructure::ipc::{IpcServer, IpcMessage, send_message};
use crate::infrastructure::sysfs::SysfsAdapter;
use crate::infrastructure::niri::NiriAdapter;
use crate::infrastructure::eww::EwwPresenter;
use clap::{Parser, Subcommand};
use tokio::time::{interval, Duration};

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
    #[deprecated(since = "0.4.0", note = "Replaced with `desktop` command for better desktop state inspection.")]
    Windows,

    /// Imprime el estado actual del escritorio (para debugging)
    Desktop,
}

#[derive(Subcommand, Debug)]
enum ActionCommands {
    /// Open a popup
    Popup {
        name: String,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(short, long)]
        keep: bool,
    },
    /// Close the current popup
    ClosePopup,
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
            handle_action(&config, action).await?;
        }
        Commands::Windows => {
            eprintln!("Warning: 'windows' command is deprecated and should only be used for debugging purposes.");
            list_windows(config).await?;
        }
        Commands::Desktop => {
            print_desktop(config).await?;
        }
    }

    Ok(())
}

async fn run_daemon(config: AppConfig) -> anyhow::Result<()> {
    let mut popup_manager = PopupManager::new();
    let ipc_server = IpcServer::new(&config.ipc.socket_path)?;
    let sys_adapter = SysfsAdapter::new("BAT0");
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path, "ui/images/icons");
    
    // El presenter ahora está abstraído. Usamos eww por defecto.
    let presenter: Box<dyn Presenter> = Box::new(EwwPresenter::new("ui"));
    
    let mut state = AppState::default();
    let mut last_emitted_state = AppState::default();
    
    let mut battery_poll = interval(Duration::from_millis(config.polling.battery_ms));
    let mut net_poll = interval(Duration::from_secs(10)); // Wifi cada 10s
    let mut audio_poll = interval(Duration::from_millis(500)); // Audio cada 0.5s
    let mut popup_check = interval(Duration::from_millis(100));
    
    let niri_socket_path = config.niri.socket_path.clone().unwrap_or_else(|| {
        std::env::var("NIRI_SOCKET").unwrap_or_else(|_| {
            let xdg_runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
            format!("{}/niri-0", xdg_runtime)
        })
    });
    
    let mut niri_events = NiriAdapter::event_listener(niri_socket_path).await?;

    loop {
        let mut state_changed = false;

        tokio::select! {
            _ = battery_poll.tick() => {
                if let Ok(bat) = sys_adapter.get_battery().await {
                    if state.system.battery != bat {
                        state.system.battery = bat;
                        state_changed = true;
                    }
                }
            }
            _ = net_poll.tick() => {
                if let Ok(net) = sys_adapter.get_network().await {
                    if state.system.network != net {
                        state.system.network = net;
                        state_changed = true;
                    }
                }
            }
            _ = audio_poll.tick() => {
                if let Ok(audio) = sys_adapter.get_audio().await {
                    if state.system.audio != audio {
                        state.system.audio = audio;
                        popup_manager.handle_action(InternalPopupAction::Open {
                            name: "volume".to_string(),
                            output: state.desktop.outputs.keys().next().cloned().unwrap_or_else(|| "eDP-1".to_string()),
                            timeout: Some(Duration::from_millis(config.popups.timeout_ms)),
                        });
                        state.ui.popup = Some(PopupState {
                            name: "volume".to_string(),
                            output: state.desktop.outputs.keys().next().cloned().unwrap_or_else(|| "eDP-1".to_string()),
                            opened_at: chrono::Utc::now().timestamp_millis() as u64,
                            timeout_ms: Some(config.popups.timeout_ms),
                        });
                        state_changed = true;
                    }
                }
            }
            Some(_) = niri_events.recv() => {
                if let Ok(desktop) = niri_adapter.get_desktop_state().await {
                    if state.desktop.outputs != desktop.outputs {
                        state.desktop.outputs = desktop.outputs;
                        state_changed = true;
                    }
                }
            }
            _ = popup_check.tick() => {
                let old_popup = popup_manager.get_state();
                popup_manager.check_timeouts();
                let new_popup = popup_manager.get_state();
                if old_popup != new_popup {
                    state.ui.popup = new_popup;
                    state_changed = true;
                }
            }
            msg = ipc_server.accept_message() => {
                match msg {
                    Some(IpcMessage::Popup { name, output, keep }) => {
                        let output = output.unwrap_or_else(|| {
                            state.desktop.outputs.keys().next().cloned().unwrap_or_else(|| "eDP-1".to_string())
                        });
                        let timeout = if keep {
                            None
                        } else {
                            Some(Duration::from_millis(config.popups.timeout_ms))
                        };
                        
                        popup_manager.handle_action(InternalPopupAction::Open { name, output, timeout });
                        state.ui.popup = popup_manager.get_state();
                        state_changed = true;
                    }
                    Some(IpcMessage::ClosePopup) => {
                        popup_manager.handle_action(InternalPopupAction::Close);
                        state.ui.popup = popup_manager.get_state();
                        state_changed = true;
                    }
                    _ => {}
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

async fn handle_action(config: &AppConfig, action: ActionCommands) -> anyhow::Result<()> {
    let msg = match action {
        ActionCommands::Popup { name, output, keep } => {
            IpcMessage::Popup { name, output, keep }
        }
        ActionCommands::ClosePopup => {
            IpcMessage::ClosePopup
        }
    };

    send_message(&config.ipc.socket_path, &msg).await?;
    Ok(())
}

#[deprecated(since = "0.4.0", note = "Replaced with `print_desktop` command for better desktop state inspection.")]
async fn list_windows(config: AppConfig) -> anyhow::Result<()> {
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path, "ui/images/icons");
    let desktop = niri_adapter.get_desktop_state().await?;
    for (output_name, output_state) in desktop.outputs {
        println!("Output: {}", output_name);
        for ws in output_state.workspaces {
            println!("  Workspace {}:", ws.id);
            for win in ws.windows {
                println!("    Window ID: {}, Title: {}, App ID: {:?}, Icon: {}, Focused: {}", 
                    win.id, win.title, win.app_id, win.app_icon, win.is_focused);
            }
        }
    }
    Ok(())
}

async fn print_desktop(config: AppConfig) -> anyhow::Result<()> {
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path, "ui/images/icons");
    let desktop = niri_adapter.get_desktop_state().await?;
    for (output_name, output_state) in desktop.outputs {
        println!("Output: {}", output_name);
        for ws in output_state.workspaces {
            println!("  [{}] {} {}:", ws.active.then(|| "x").unwrap_or(" "), ws.id, ws.name.as_deref().unwrap_or(""));
            for win in ws.windows {
                println!("    [{}] {} {}\n      {}\n      {}", 
                    win.is_focused.then(|| "x").unwrap_or(" "), win.id, win.app_id.as_deref().unwrap_or("None"), 
                    win.title,
                    win.app_icon);
            }
        }
    }
    Ok(())
}
