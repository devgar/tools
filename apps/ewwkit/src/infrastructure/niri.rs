use crate::domain::{WindowManager, Workspace};
use async_trait::async_trait;
use tokio::process::Command;
use serde_json::Value;

pub struct NiriAdapter;

#[async_trait]
impl WindowManager for NiriAdapter {
    async fn get_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        let output = Command::new("niri")
            .args(["msg", "--json", "workspaces"])
            .output()
            .await?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!("Niri command failed"));
        }

        let workspaces: Vec<Value> = serde_json::from_slice(&output.stdout)?;
        
        let result = workspaces.into_iter().map(|w| Workspace {
            id: w["idx"].as_u64().unwrap_or(0) as u32,
            active: w["is_active"].as_bool().unwrap_or(false),
            windows_count: 0,
            output: w["output"].as_str().unwrap_or("").to_string(),
        }).collect();

        Ok(result)
    }

    async fn get_focused_window(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}
