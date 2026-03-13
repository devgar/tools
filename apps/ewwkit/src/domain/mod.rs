use serde::Serialize;
use async_trait::async_trait;

#[derive(Debug, Serialize, Clone, Default)]
pub struct SystemState {
    pub battery: BatteryState,
    pub network: NetworkState,
    pub desktop: DesktopState,
    pub audio: AudioState,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct BatteryState {
    pub level: u8,
    pub status: String,
    pub icon: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct NetworkState {
    pub wifi_ssid: String,
    pub signal: u8,
    pub icon: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct DesktopState {
    pub workspaces: Vec<Workspace>,
    pub focused_window: Option<String>,
}

#[derive(Debug, Serialize, Clone, Default, serde::Deserialize)]
pub struct Workspace {
    pub id: u32,
    pub active: bool,
    pub windows_count: u32,
    pub output: String,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct AudioState {
    pub volume: u8,
    pub muted: bool,
}

#[async_trait]
pub trait WindowManager: Send + Sync {
    async fn get_workspaces(&self) -> anyhow::Result<Vec<Workspace>>;
    async fn get_focused_window(&self) -> anyhow::Result<Option<String>>;
}

#[async_trait]
pub trait SystemProvider: Send + Sync {
    async fn get_battery(&self) -> anyhow::Result<BatteryState>;
    async fn get_network(&self) -> anyhow::Result<NetworkState>;
    async fn get_audio(&self) -> anyhow::Result<AudioState>;
}
