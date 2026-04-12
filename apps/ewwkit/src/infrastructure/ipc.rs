use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Wire format sent over the IPC socket (daemon receives, client sends).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum VolumeAction {
    Up { step: u8 },
    Down { step: u8 },
    Set { percent: u8 },
    Mute,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BrightnessAction {
    Up { step: u8 },
    Down { step: u8 },
    Set { percent: u8 },
}

/// CLI subcommands for `ewwkit action volume <subcommand>`.
#[derive(Subcommand, Debug, Clone)]
pub enum VolumeCommands {
    /// Increase volume by step percent
    Up {
        #[arg(short, long)]
        step: Option<u8>,
    },
    /// Decrease volume by step percent
    Down {
        #[arg(short, long)]
        step: Option<u8>,
    },
    /// Set volume to an absolute percentage (0–100)
    Set { percent: u8 },
    /// Toggle mute
    Mute,
}

/// CLI subcommands for `ewwkit action brightness <subcommand>`.
#[derive(Subcommand, Debug, Clone)]
pub enum BrightnessCommands {
    /// Increase brightness by step percent
    Up {
        #[arg(short, long)]
        step: Option<u8>,
    },
    /// Decrease brightness by step percent
    Down {
        #[arg(short, long)]
        step: Option<u8>,
    },
    /// Set brightness to an absolute percentage (0–100)
    Set { percent: u8 },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum IpcMessage {
    Popup {
        name: String,
        output: Option<String>,
        keep: bool,
    },
    ClosePopup,
    GetState,
    Volume(VolumeAction),
    Brightness(BrightnessAction),
}

pub async fn send_message(socket_path: &str, msg: &IpcMessage) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(socket_path).await?;
    let encoded = serde_json::to_vec(msg)?;
    stream.write_all(&encoded).await?;
    Ok(())
}

pub struct IpcServer {
    listener: UnixListener,
}

impl IpcServer {
    pub fn new(socket_path: &str) -> anyhow::Result<Self> {
        if Path::new(socket_path).exists() {
            let _ = fs::remove_file(socket_path);
        }
        let std_listener = std::os::unix::net::UnixListener::bind(socket_path)?;
        std_listener.set_nonblocking(true)?;
        let listener = UnixListener::from_std(std_listener)?;
        Ok(Self { listener })
    }

    pub async fn accept_message(&self) -> Option<IpcMessage> {
        match self.listener.accept().await {
            Ok((mut stream, _)) => {
                let mut buffer = Vec::new();
                let _ = stream.read_to_end(&mut buffer).await;
                serde_json::from_slice(&buffer).ok()
            }
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: &IpcMessage) -> IpcMessage {
        let json = serde_json::to_string(msg).expect("serialize");
        serde_json::from_str(&json).expect("deserialize")
    }

    #[test]
    fn popup_with_output_roundtrip() {
        let msg = IpcMessage::Popup { name: "volume".to_string(), output: Some("HDMI-1".to_string()), keep: false };
        match roundtrip(&msg) {
            IpcMessage::Popup { name, output, keep } => {
                assert_eq!(name, "volume");
                assert_eq!(output.as_deref(), Some("HDMI-1"));
                assert!(!keep);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn popup_without_output_roundtrip() {
        let msg = IpcMessage::Popup { name: "brightness".to_string(), output: None, keep: true };
        match roundtrip(&msg) {
            IpcMessage::Popup { name, output, keep } => {
                assert_eq!(name, "brightness");
                assert!(output.is_none());
                assert!(keep);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn close_popup_roundtrip() {
        assert!(matches!(roundtrip(&IpcMessage::ClosePopup), IpcMessage::ClosePopup));
    }

    #[test]
    fn get_state_roundtrip() {
        assert!(matches!(roundtrip(&IpcMessage::GetState), IpcMessage::GetState));
    }

    #[test]
    fn volume_actions_roundtrip() {
        let cases = [
            IpcMessage::Volume(VolumeAction::Up { step: 5 }),
            IpcMessage::Volume(VolumeAction::Down { step: 3 }),
            IpcMessage::Volume(VolumeAction::Set { percent: 80 }),
            IpcMessage::Volume(VolumeAction::Mute),
        ];
        for msg in &cases {
            let rt = roundtrip(msg);
            assert_eq!(serde_json::to_string(&rt).unwrap(), serde_json::to_string(msg).unwrap());
        }
    }

    #[test]
    fn brightness_actions_roundtrip() {
        let cases = [
            IpcMessage::Brightness(BrightnessAction::Up { step: 10 }),
            IpcMessage::Brightness(BrightnessAction::Down { step: 5 }),
            IpcMessage::Brightness(BrightnessAction::Set { percent: 50 }),
        ];
        for msg in &cases {
            let rt = roundtrip(msg);
            assert_eq!(serde_json::to_string(&rt).unwrap(), serde_json::to_string(msg).unwrap());
        }
    }
}
