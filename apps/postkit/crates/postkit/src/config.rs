use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub accounts: HashMap<String, AccountConfig>,
    /// URL base del daemon para el subcomando `schedule`.
    pub daemon_url: Option<String>,
    pub daemon_api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum AccountConfig {
    Bluesky {
        handle: String,
        app_password: String,
    },
    X {
        api_key: String,
        api_secret: String,
        access_token: String,
        access_token_secret: String,
    },
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("reading {}: {}", path.display(), e))?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }
}
