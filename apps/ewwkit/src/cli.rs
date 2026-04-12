use crate::config::AppConfig;
use crate::domain::WindowManager;
use crate::infrastructure::ipc::{
    BrightnessAction, BrightnessCommands, IpcMessage, VolumeAction, VolumeCommands, send_message,
};
use crate::infrastructure::niri::NiriAdapter;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
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
pub enum ActionCommands {
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

pub async fn handle_action(config: &AppConfig, action: ActionCommands) -> anyhow::Result<()> {
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

pub async fn print_desktop(config: AppConfig) -> anyhow::Result<()> {
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
