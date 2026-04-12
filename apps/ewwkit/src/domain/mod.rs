use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct AppState {
    pub system: SystemState,
    pub desktop: DesktopState,
    pub ui: UiState,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct UiState {
    pub popup: Option<PopupState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct SystemState {
    pub battery: BatteryState,
    pub network: NetworkState,
    pub audio: AudioState,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct BatteryState {
    pub level: u8,
    pub status: String,
    pub icon: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct NetworkState {
    pub wifi_ssid: String,
    pub signal: u8,
    pub icon: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct DesktopState {
    pub outputs: BTreeMap<String, OutputState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct OutputState {
    pub workspaces: Vec<WorkspaceState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct WorkspaceState {
    pub id: u64,
    pub idx: u32,
    pub name: Option<String>,
    pub active: bool,
    pub windows: Vec<WindowState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct WindowState {
    pub id: u64,
    pub title: String,
    pub app_id: Option<String>,
    pub is_focused: bool,
    pub app_icon: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct AudioState {
    pub volume: u8,
    pub muted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct PopupState {
    pub name: String,
    pub output: String,
    pub opened_at: u64,
    pub timeout_ms: Option<u64>,
}

/// A module that owns a slice of `AppState` identified by `path()`.
///
/// `path` is a dotted key naming the state field this provider owns,
/// e.g. `"system.battery"`, `"system.audio"`, `"system.network"`.
/// It is used for logging and future action routing.
#[async_trait]
pub trait StateProvider<S: Clone + Send + 'static>: Send + Sync {
    /// Dotted path in the state tree this provider owns (e.g. `"system.battery"`).
    fn path(&self) -> &'static str;
    /// Fetch the current state once (used at daemon startup).
    async fn init(&self) -> anyhow::Result<S>;
    /// Returns a channel that emits a new value whenever state changes.
    fn watch(&self) -> mpsc::Receiver<S>;
}

#[async_trait]
pub trait Presenter: Send + Sync {
    async fn update_state(&self, state: &AppState) -> anyhow::Result<()>;
}

#[async_trait]
pub trait WindowManager: Send + Sync {
    async fn get_desktop_state(&self) -> anyhow::Result<DesktopState>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_serializes_with_outputs_keyed_by_output_name() {
        let mut state = AppState {
            system: SystemState {
                battery: BatteryState {
                    level: 87,
                    status: "Discharging".to_string(),
                    icon: "battery-icon".to_string(),
                },
                network: NetworkState {
                    wifi_ssid: "my-wifi".to_string(),
                    signal: 72,
                    icon: "wifi-icon".to_string(),
                },
                audio: AudioState {
                    volume: 45,
                    muted: false,
                },
            },
            desktop: DesktopState::default(),
            ui: UiState {
                popup: Some(PopupState {
                    name: "dashboard".to_string(),
                    output: "HDMI-A-1".to_string(),
                    opened_at: 42,
                    timeout_ms: Some(3_000),
                }),
            },
        };

        state.desktop.outputs.insert(
            "HDMI-A-1".to_string(),
            OutputState {
                workspaces: vec![WorkspaceState {
                    id: 3,
                    idx: 3,
                    active: true,
                    name: None,
                    windows: vec![WindowState {
                        id: 10,
                        title: "Terminal".to_string(),
                        app_id: Some("kitty".to_string()),
                        is_focused: true,
                        app_icon: "kitty.svg".to_string(),
                    }],
                }],
            },
        );

        let json = serde_json::to_value(&state).expect("state must serialize");

        assert!(json.get("system").is_some());
        assert!(json.get("desktop").is_some());
        assert!(json.get("ui").is_some());

        let outputs = json
            .get("desktop")
            .and_then(|desktop| desktop.get("outputs"))
            .expect("desktop.outputs must exist");

        assert!(outputs.is_object(), "desktop.outputs must be a JSON object");
        assert!(
            outputs.get("HDMI-A-1").is_some(),
            "desktop.outputs must contain key per output name"
        );

        let output_state = outputs
            .get("HDMI-A-1")
            .expect("output key must map to an output state");
        assert!(
            output_state.get("workspaces").is_some(),
            "output state must contain workspaces field"
        );
        assert!(
            output_state.get("name").is_none(),
            "output state must not contain legacy name field"
        );
    }
}
