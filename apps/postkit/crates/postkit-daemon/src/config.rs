use std::{collections::HashMap, path::Path};

use anyhow::Context as _;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    pub db_path: String,
    pub listen: String,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    pub accounts_config: String,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,
    /// Omit to disable authentication (useful for local dev).
    pub api_key: Option<String>,
    /// Redis URL. Omit or leave unreachable to fall back to the in-process queue.
    pub redis_url: Option<String>,
}

fn default_poll() -> u64 { 30 }
fn default_max_attempts() -> u32 { 3 }
fn default_retry_delay() -> u64 { 60 }

impl DaemonConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("config file not found: '{}'", path.display()))?;
        toml::from_str(&text).with_context(|| format!("failed to parse: '{}'", path.display()))
    }
}

// ─── App credentials (shared across accounts of the same provider) ────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum AppConfig {
    X {
        api_key: String,
        api_secret: String,
    },
    Meta {
        /// Meta App ID and secret (optional; required for user token rotation).
        #[serde(default)]
        app_id: Option<String>,
        #[serde(default)]
        app_secret: Option<String>,
    },
}

// ─── Account credentials (per account) ───────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum AccountConfig {
    Bluesky {
        handle: String,
        app_password: String,
    },
    X {
        /// Reference to an entry in [apps].
        app: String,
        access_token: String,
        access_token_secret: String,
    },
    FacebookPage {
        /// Reference to an entry in [apps] (meta type).
        #[allow(dead_code)]
        app: String,
        page_id: String,
        page_access_token: String,
    },
    Instagram {
        /// Reference to an entry in [apps] (meta type).
        #[allow(dead_code)]
        app: String,
        ig_user_id: String,
        access_token: String,
    },
}

#[derive(Deserialize)]
struct AccountsFile {
    #[serde(default)]
    pub apps: HashMap<String, AppConfig>,
    #[serde(default)]
    pub accounts: HashMap<String, AccountConfig>,
}

pub fn load_accounts(
    path: &str,
) -> anyhow::Result<(HashMap<String, AppConfig>, HashMap<String, AccountConfig>)> {
    let text = std::fs::read_to_string(path)?;
    let f: AccountsFile = toml::from_str(&text)?;
    Ok((f.apps, f.accounts))
}
