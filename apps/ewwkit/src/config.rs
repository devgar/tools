use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;
use std::env;
use std::time::Duration;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub popups: PopupsConfig,
    pub niri: NiriConfig,
    pub ipc: IpcConfig,
    pub controls: ControlsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ControlsConfig {
    pub volume_step: u8,
    pub brightness_step: u8,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PopupsConfig {
    #[serde(rename = "timeout_ms", with = "duration_ms")]
    pub timeout: Duration,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NiriConfig {
    pub socket_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct IpcConfig {
    pub socket_path: String,
}

impl AppConfig {
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let s = Config::builder()
            // Configuración por defecto
            .set_default("popups.timeout_ms", 3000u64)?
            .set_default("niri.socket_path", None::<String>)?
            .set_default("ipc.socket_path", "/tmp/ewwkit.sock")?
            .set_default("controls.volume_step", 5u64)?
            .set_default("controls.brightness_step", 5u64)?
            // Carga de archivo config/default.toml (opcional)
            .add_source(File::with_name("config/default").required(false))
            .add_source(File::with_name(&format!("config/{}", run_mode)).required(false))
            // Carga de variables de entorno con prefijo EWWKIT_
            .add_source(Environment::with_prefix("EWWKIT").separator("__"))
            .build()?;

        s.try_deserialize()
    }
}

mod duration_ms {
    use serde::{Deserialize, Deserializer};
    use std::time::Duration;

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
    }
}
