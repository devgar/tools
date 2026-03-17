use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct SystemState {
    pub system: SystemMetrics,
    pub desktop: DesktopState,
    pub ui: UiState,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct UiState {
    pub popup: Option<PopupState>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct SystemMetrics {
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
    pub outputs: Vec<Output>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Output {
    pub name: String,
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Workspace {
    pub id: u32,
    pub active: bool,
    pub windows: Vec<Window>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Window {
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
    async fn get_desktop_state(&self) -> anyhow::Result<DesktopState>;
}

#[async_trait]
pub trait SystemProvider: Send + Sync {
    async fn get_battery(&self) -> anyhow::Result<BatteryState>;
    async fn get_network(&self) -> anyhow::Result<NetworkState>;
    async fn get_audio(&self) -> anyhow::Result<AudioState>;
}
