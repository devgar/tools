use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub accounts: HashMap<String, AccountConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum AccountConfig {
    Bluesky {
        /// Ej: "devgar.bsky.social"
        handle: String,
        /// App password generada en https://bsky.app/settings/app-passwords
        /// NUNCA uses tu contraseña principal.
        app_password: String,
    },
    // Futuros:
    // X { ... }, MetaPage { ... }, etc.
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {}", path.display(), e))?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }
}
