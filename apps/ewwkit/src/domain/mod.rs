use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct SystemState {
    pub battery: BatteryState,
    pub network: NetworkState,
    pub desktop: DesktopState,
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
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub focused_window_id: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Workspace {
    pub id: u32,
    pub active: bool,
    pub windows_count: u32,
    pub output: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Window {
    pub id: u64,
    pub title: String,
    pub app_id: Option<String>,
    pub workspace_id: u64,
    pub is_focused: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct AudioState {
    pub volume: u8,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PresentationAction {
    Open(String),
    Close(String),
    Toggle(String),
}

#[async_trait]
pub trait Presenter: Send + Sync {
    /// Update the overall UI state
    async fn update_state(&self, state: &SystemState) -> anyhow::Result<()>;
    
    /// Handle a specific UI action
    async fn execute_action(&self, action: PresentationAction) -> anyhow::Result<()>;
}

#[async_trait]
pub trait WindowManager: Send + Sync {
    async fn get_workspaces(&self) -> anyhow::Result<Vec<Workspace>>;
    async fn get_windows(&self) -> anyhow::Result<Vec<Window>>;
    async fn get_focused_window_id(&self) -> anyhow::Result<Option<u64>>;
}

#[async_trait]
pub trait SystemProvider: Send + Sync {
    async fn get_battery(&self) -> anyhow::Result<BatteryState>;
    async fn get_network(&self) -> anyhow::Result<NetworkState>;
    async fn get_audio(&self) -> anyhow::Result<AudioState>;
}
