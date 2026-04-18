use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    pub db_path: String,
    pub listen: String,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    pub accounts_config: String,
}

fn default_poll() -> u64 {
    30
}

impl DaemonConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
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

#[derive(Deserialize)]
struct AccountsFile {
    #[serde(default)]
    pub accounts: HashMap<String, AccountConfig>,
}

pub fn load_accounts(path: &str) -> anyhow::Result<HashMap<String, AccountConfig>> {
    let text = std::fs::read_to_string(path)?;
    let f: AccountsFile = toml::from_str(&text)?;
    Ok(f.accounts)
}
