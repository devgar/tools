mod application;
mod cli;
mod config;
mod domain;
mod infrastructure;
mod popup;
mod state;
#[cfg(test)]
mod test_utils;

use crate::application::handle_event;
use crate::cli::{Cli, Commands};
use crate::config::AppConfig;
use crate::domain::{AppState, Presenter, StateProvider};
use crate::infrastructure::audio;
use crate::infrastructure::battery;
use crate::infrastructure::brightness;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::eww::EwwPresenter;
use crate::infrastructure::ipc::IpcServer;
use crate::infrastructure::niri::NiriAdapter;
use crate::infrastructure::wifi;
use crate::popup::PopupManager;
use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = AppConfig::new()?;

    match cli.command {
        Commands::Daemon => run_daemon(config).await?,
        Commands::Action { action } => cli::handle_action(&config, action).await?,
        Commands::Desktop => cli::print_desktop(config).await?,
    }

    Ok(())
}

async fn run_daemon(config: AppConfig) -> anyhow::Result<()> {
    let ipc_server = IpcServer::new(&config.ipc.socket_path)?;
    let niri_adapter = NiriAdapter::new(config.niri.socket_path.as_deref(), "ui/images/icons");
    let presenter: Box<dyn Presenter> = Box::new(EwwPresenter {});

    let mut state = AppState::default();
    let mut last_emitted_state = AppState::default();
    let mut popup_manager = PopupManager::new();

    let battery_provider    = battery::create_battery_provider();
    let audio_monitor       = audio::create_monitor();
    let wifi_provider       = wifi::create_wifi_provider();
    let brightness_provider = brightness::create_brightness_provider();

    eprintln!("ewwkit [{}]: initializing", battery_provider.path());
    eprintln!("ewwkit [{}]: initializing", audio_monitor.path());
    eprintln!("ewwkit [{}]: initializing", wifi_provider.path());
    eprintln!("ewwkit [{}]: initializing", brightness_provider.path());

    let (bat, audio, net, brightness) = tokio::join!(
        battery_provider.init(),
        audio_monitor.init(),
        wifi_provider.init(),
        brightness_provider.init(),
    );

    if let Ok(b) = bat        { state.system.battery    = b; }
    if let Ok(a) = audio      { state.system.audio      = a; }
    if let Ok(n) = net        { state.system.network    = n; }
    if let Ok(b) = brightness { state.system.brightness = b; }

    let brightness_rx = brightness_provider.watch();

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
        brightness_rx,
        niri_events,
        ipc_server,
    );

    loop {
        let event = event_bus.next().await;
        let changed =
            handle_event(event, &mut state, &mut popup_manager, &config, &niri_adapter).await;
        if changed && state != last_emitted_state {
            let _ = presenter.update_state(&state).await;
            last_emitted_state = state.clone();
        }
    }
}
