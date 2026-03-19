use serde::{Deserialize, Serialize};
use std::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub enum IpcMessage {
    Popup {
        name: String,
        output: Option<String>,
        action: PopupAction,
    },
    GetState,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PopupAction {
    Open,
    Close,
    KeepAlive,
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
