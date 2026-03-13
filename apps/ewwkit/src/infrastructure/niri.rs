use crate::domain::{WindowManager, Workspace};
use async_trait::async_trait;
use tokio::process::Command;
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct NiriAdapter;

impl NiriAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Crea un canal que recibe notificaciones de eventos de Niri
    pub async fn event_listener() -> anyhow::Result<tokio::sync::mpsc::Receiver<()>> {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        
        tokio::spawn(async move {
            let mut child = Command::new("niri")
                .args(["msg", "--json", "event-stream"])
                .stdout(Stdio::piped())
                .spawn()
                .expect("Failed to spawn niri event-stream");

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let mut lines = BufReader::new(stdout).lines();

            while let Ok(Some(_)) = lines.next_line().await {
                let _ = tx.send(()).await;
            }
        });

        Ok(rx)
    }
}

#[async_trait]
impl WindowManager for NiriAdapter {
    async fn get_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        // Obtener workspaces
        let ws_output = Command::new("niri")
            .args(["msg", "--json", "workspaces"])
            .output()
            .await?;
        
        let ws_json: Vec<Value> = serde_json::from_slice(&ws_output.stdout)?;

        // Obtener ventanas para contar cuántas hay en cada workspace
        let win_output = Command::new("niri")
            .args(["msg", "--json", "windows"])
            .output()
            .await?;
        
        let win_json: Vec<Value> = serde_json::from_slice(&win_output.stdout).unwrap_or_default();

        let workspaces = ws_json.into_iter().map(|w| {
            let internal_id = w["id"].as_u64().unwrap_or(0);
            let idx = w["idx"].as_u64().unwrap_or(0) as u32;
            let active = w["is_active"].as_bool().unwrap_or(false);
            let output_name = w["output"].as_str().unwrap_or("").to_string();
            
            let count = win_json.iter()
                .filter(|win| win["workspace_id"].as_u64() == Some(internal_id))
                .count() as u32;

            Workspace {
                id: idx,
                active,
                windows_count: count,
                output: output_name,
            }
        }).collect();

        Ok(workspaces)
    }

    async fn get_focused_window(&self) -> anyhow::Result<Option<String>> {
        let output = Command::new("niri")
            .args(["msg", "--json", "windows"])
            .output()
            .await?;
        
        let windows: Vec<Value> = serde_json::from_slice(&output.stdout)?;
        let focused = windows.into_iter()
            .find(|w| w["is_focused"].as_bool().unwrap_or(false))
            .map(|w| w["title"].as_str().unwrap_or("").to_string());

        Ok(focused)
    }
}
