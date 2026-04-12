mod application;
mod config;
mod domain;
mod infrastructure;
mod popup;
mod state;

use crate::application::handle_event;
use crate::config::AppConfig;
use crate::domain::{AppState, Presenter, StateProvider, WindowManager};
use crate::infrastructure::audio;
use crate::infrastructure::battery;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::ipc::{
    BrightnessAction, BrightnessCommands, IpcMessage, VolumeAction, VolumeCommands, send_message,
};
use crate::infrastructure::ipc::{IpcServer};
use crate::infrastructure::niri::NiriAdapter;
use crate::infrastructure::eww::EwwPresenter;
use crate::infrastructure::wifi;
use crate::popup::PopupManager;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the monitoring daemon
    Daemon,
    /// Send an action to the running daemon
    Action {
        #[command(subcommand)]
        action: ActionCommands,
    },
    /// Print the current desktop state (for debugging)
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
    /// Control audio volume
    Volume {
        #[command(subcommand)]
        action: VolumeCommands,
    },
    /// Control screen brightness
    Brightness {
        #[command(subcommand)]
        action: BrightnessCommands,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::new()?;

    match cli.command {
        Commands::Daemon => run_daemon(config).await?,
        Commands::Action { action } => handle_action(&config, action).await?,
        Commands::Desktop => print_desktop(config).await?,
    }

    Ok(())
}

async fn run_daemon(config: AppConfig) -> anyhow::Result<()> {
    let ipc_server = IpcServer::new(&config.ipc.socket_path)?;
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path, "ui/images/icons");
    let presenter: Box<dyn Presenter> = Box::new(EwwPresenter{});

    let mut state = AppState::default();
    let mut last_emitted_state = AppState::default();
    let mut popup_manager = PopupManager::new();

    let battery_provider = battery::create_battery_provider();
    eprintln!("ewwkit [{}]: initializing", battery_provider.path());
    if let Ok(bat) = battery_provider.init().await {
        state.system.battery = bat;
    }

    let audio_monitor = audio::create_monitor();
    eprintln!("ewwkit [{}]: initializing", audio_monitor.path());
    if let Ok(audio) = audio_monitor.init().await {
        state.system.audio = audio;
    }

    let wifi_provider = wifi::create_wifi_provider();
    eprintln!("ewwkit [{}]: initializing", wifi_provider.path());
    if let Ok(net) = wifi_provider.init().await {
        state.system.network = net;
    }

    let niri_socket_path = config.niri.socket_path.clone().unwrap_or_else(|| {
        std::env::var("NIRI_SOCKET").unwrap_or_else(|_| {
            let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
                .unwrap_or_else(|_| "/run/user/1000".to_string());
            format!("{}/niri-0", xdg_runtime)
        })
    });

    let niri_events = NiriAdapter::event_listener(niri_socket_path).await?;

    let mut event_bus = EventBus::new(
        battery_provider.watch(),
        audio_monitor.watch(),
        wifi_provider.watch(),
        niri_events,
        ipc_server,
    );

    loop {
        let event = event_bus.next().await;
        let changed = handle_event(event, &mut state, &mut popup_manager, &config, &niri_adapter).await;
        if changed && state != last_emitted_state {
            let _ = presenter.update_state(&state).await;
            last_emitted_state = state.clone();
        }
    }
}

async fn handle_action(config: &AppConfig, action: ActionCommands) -> anyhow::Result<()> {
    let msg = match action {
        ActionCommands::Popup { name, output, keep } => IpcMessage::Popup { name, output, keep },
        ActionCommands::ClosePopup => IpcMessage::ClosePopup,
        ActionCommands::Volume { action } => IpcMessage::Volume(match action {
            VolumeCommands::Up { step } => VolumeAction::Up {
                step: step.unwrap_or(config.controls.volume_step),
            },
            VolumeCommands::Down { step } => VolumeAction::Down {
                step: step.unwrap_or(config.controls.volume_step),
            },
            VolumeCommands::Set { percent } => VolumeAction::Set { percent },
            VolumeCommands::Mute => VolumeAction::Mute,
        }),
        ActionCommands::Brightness { action } => IpcMessage::Brightness(match action {
            BrightnessCommands::Up { step } => BrightnessAction::Up {
                step: step.unwrap_or(config.controls.brightness_step),
            },
            BrightnessCommands::Down { step } => BrightnessAction::Down {
                step: step.unwrap_or(config.controls.brightness_step),
            },
            BrightnessCommands::Set { percent } => BrightnessAction::Set { percent },
        }),
    };

    send_message(&config.ipc.socket_path, &msg).await?;
    Ok(())
}

async fn print_desktop(config: AppConfig) -> anyhow::Result<()> {
    let niri_adapter = NiriAdapter::new(&config.niri.socket_path, "ui/images/icons");
    let desktop = niri_adapter.get_desktop_state().await?;
    for (output_name, output_state) in desktop.outputs {
        println!("Output: {}", output_name);
        for ws in output_state.workspaces {
            println!(
                "  [{}] {} {}:",
                ws.active.then(|| "x").unwrap_or(" "),
                ws.id,
                ws.name.as_deref().unwrap_or("")
            );
            for win in ws.windows {
                println!(
                    "    [{}] {} {}\n      {}\n      {}",
                    win.is_focused.then(|| "x").unwrap_or(" "),
                    win.id,
                    win.app_id.as_deref().unwrap_or("None"),
                    win.title,
                    win.app_icon
                );
            }
        }
    }
    Ok(())
}
