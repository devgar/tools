use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub accounts: HashMap<String, AccountConfig>,
    /// Base URL of the daemon for the `schedule` subcommand. Default: http://localhost:8080.
    pub daemon_url: Option<String>,
    /// API key of the daemon (X-Api-Key). Default: config daemon_api_key.
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
    FacebookPage {
        page_id: String,
        page_access_token: String,
    },
    Instagram {
        ig_user_id: String,
        access_token: String,
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
