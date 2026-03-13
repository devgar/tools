use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use anyhow::Result;

pub enum PopupAction {
    Open(String),
    Close(String),
    KeepAlive(String),
}

pub struct PopupManager {
    current_popup: Option<(String, Instant)>,
    timeout: Duration,
    tx: mpsc::Sender<String>, // Para enviar comandos de cierre a EWW
}

impl PopupManager {
    pub fn new(timeout_ms: u64, tx: mpsc::Sender<String>) -> Self {
        Self {
            current_popup: None,
            timeout: Duration::from_millis(timeout_ms),
            tx,
        }
    }

    pub async fn handle_action(&mut self, action: PopupAction) -> Result<()> {
        match action {
            PopupAction::Open(name) => {
                // Exclusividad
                if let Some((current_name, _)) = &self.current_popup {
                    if *current_name != name {
                        let n = current_name.clone();
                        self.close_popup(n).await?;
                    }
                }
                self.open_popup(name).await?;
            }
            PopupAction::Close(name) => {
                self.close_popup(name).await?;
            }
            PopupAction::KeepAlive(name) => {
                if let Some((current_name, expiry)) = &mut self.current_popup {
                    if *current_name == name {
                        *expiry = Instant::now() + self.timeout;
                    }
                }
            }
        }
        Ok(())
    }

    async fn open_popup(&mut self, name: String) -> Result<String> {
        self.current_popup = Some((name.clone(), Instant::now() + self.timeout));
        Ok(format!("open {}", name))
    }

    async fn close_popup(&mut self, name: String) -> Result<()> {
        self.current_popup = None;
        self.tx.send(format!("close {}", name)).await?;
        Ok(())
    }

    pub async fn check_timeouts(&mut self) -> Result<()> {
        if let Some((name, expiry)) = &self.current_popup {
            if Instant::now() > *expiry {
                let n = name.clone();
                self.close_popup(n).await?;
            }
        }
        Ok(())
    }
}
