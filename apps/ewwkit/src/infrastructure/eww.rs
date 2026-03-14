use crate::domain::{Presenter, PresentationAction, SystemState};
use async_trait::async_trait;
use std::process::Command;
use serde_json;

pub struct EwwPresenter {
    config_dir: String,
}

impl EwwPresenter {
    pub fn new(config_dir: &str) -> Self {
        Self {
            config_dir: config_dir.to_string(),
        }
    }
}

#[async_trait]
impl Presenter for EwwPresenter {
    async fn update_state(&self, state: &SystemState) -> anyhow::Result<()> {
        let json = serde_json::to_string(state)?;
        println!("{}", json);
        Ok(())
    }

    async fn execute_action(&self, action: PresentationAction) -> anyhow::Result<()> {
        let (cmd, window) = match action {
            PresentationAction::Open(name) => ("open", name),
            PresentationAction::Close(name) => ("close", name),
            PresentationAction::Toggle(name) => ("open", name), // Eww doesn't have a native toggle via CLI that works reliably without state checks, but we can handle it
        };

        let mut process = Command::new("eww")
            .args(["-c", &self.config_dir, cmd, &window])
            .spawn()?;
        
        process.wait()?;
        Ok(())
    }
}
