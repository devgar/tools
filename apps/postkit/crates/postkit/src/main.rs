mod config;

use anyhow::{Context, Result};
use reqwest;
use clap::{Parser, Subcommand};
use postkit_core::{Provider, SourcePost};
use postkit_providers_bluesky::Bluesky;
use postkit_providers_x::X;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::{AccountConfig, Config};

#[derive(Parser)]
#[command(name = "postkit", about = "Multi-platform social media scheduler")]
struct Cli {
    /// Config file path. Accepts `~/`.
    #[arg(long, default_value = "~/.config/postkit/config.toml")]
    config: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List configured accounts.
    Accounts,

    /// Verify credentials for one or all accounts.
    Verify { account: Option<String> },

    /// Compose a post and emit the JSON plan without executing it.
    Compose {
        /// TOML with the SourcePost.
        post: PathBuf,
        /// Target accounts. If omitted, all.
        #[arg(long, num_args = 0..)]
        targets: Vec<String>,
    },

    /// Compose and publish immediately (without scheduling).
    Publish {
        post: PathBuf,
        #[arg(long, num_args = 0..)]
        targets: Vec<String>,
    },

    /// Schedule a post for future publication via the daemon.
    Schedule {
        /// TOML with the SourcePost.
        post: PathBuf,
        /// Target accounts. If omitted, all.
        #[arg(long, num_args = 0..)]
        targets: Vec<String>,
        /// When to publish (RFC 3339, e.g., 2026-04-21T10:00:00Z).
        #[arg(long)]
        at: String,
        /// Base URL of the daemon. Default: config daemon_url or http://localhost:8080.
        #[arg(long)]
        daemon: Option<String>,
        /// API key of the daemon (X-Api-Key). Default: config daemon_api_key.
        #[arg(long)]
        api_key: Option<String>,
    },
}

fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(s)
}

fn build_providers(cfg: &Config) -> HashMap<String, Arc<dyn Provider>> {
    let mut out: HashMap<String, Arc<dyn Provider>> = HashMap::new();
    for (id, acc) in &cfg.accounts {
        match acc {
            AccountConfig::Bluesky { handle, app_password } => {
                out.insert(
                    id.clone(),
                    Arc::new(Bluesky::new(id.clone(), handle.clone(), app_password.clone())),
                );
            }
            AccountConfig::X {
                api_key,
                api_secret,
                access_token,
                access_token_secret,
            } => {
                out.insert(
                    id.clone(),
                    Arc::new(X::new(
                        id.clone(),
                        api_key.clone(),
                        api_secret.clone(),
                        access_token.clone(),
                        access_token_secret.clone(),
                    )),
                );
            }
        }
    }
    out
}

fn resolve_targets(
    providers: &HashMap<String, Arc<dyn Provider>>,
    targets: &[String],
) -> Result<Vec<String>> {
    if targets.is_empty() {
        return Ok(providers.keys().cloned().collect());
    }
    for t in targets {
        if !providers.contains_key(t) {
            anyhow::bail!("unknown account: {t}");
        }
    }
    Ok(targets.to_vec())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = expand_tilde(&cli.config);
    let cfg = Config::load(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;
    let providers = build_providers(&cfg);

    if providers.is_empty() {
        eprintln!("⚠ no accounts configured in {}", config_path.display());
    }

    match cli.cmd {
        Cmd::Accounts => {
            for (id, p) in &providers {
                println!("{:<20} {:?}", id, p.kind());
            }
        }

        Cmd::Verify { account } => {
            let ids: Vec<String> = match account {
                Some(a) => vec![a],
                None => providers.keys().cloned().collect(),
            };
            for id in ids {
                let p = providers
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("unknown account: {id}"))?;
                match p.verify().await {
                    Ok(info) => println!("✓ {id} → @{}", info.handle),
                    Err(e) => println!("✗ {id} → {e}"),
                }
            }
        }

        Cmd::Compose { post, targets } => {
            let source = load_post(&post)?;
            let ids = resolve_targets(&providers, &targets)?;
            let mut plans = Vec::new();
            for id in ids {
                let p = providers.get(&id).unwrap();
                plans.push(p.compose(&source)?);
            }
            println!("{}", serde_json::to_string_pretty(&plans)?);
        }

        Cmd::Schedule { post, targets, at, daemon, api_key } => {
            let source = load_post(&post)?;
            let ids = resolve_targets(&providers, &targets)?;

            let daemon_url = daemon
                .or_else(|| cfg.daemon_url.clone())
                .unwrap_or_else(|| "http://localhost:8080".to_string());
            let api_key = api_key.or_else(|| cfg.daemon_api_key.clone());

            let scheduled_at: chrono::DateTime<chrono::Utc> =
                chrono::DateTime::parse_from_rfc3339(&at)
                    .with_context(|| format!("invalid date: {at}"))?
                    .into();

            let client = reqwest::Client::new();
            for id in ids {
                let body = serde_json::json!({
                    "account_id": id,
                    "source_post": source,
                    "scheduled_at": scheduled_at,
                });
                let mut req = client.post(format!("{daemon_url}/schedule")).json(&body);
                if let Some(ref key) = api_key {
                    req = req.header("X-Api-Key", key);
                }
                let resp = req
                    .send()
                    .await
                    .with_context(|| format!("contacting daemon at {daemon_url}"))?;
                let status = resp.status();
                if status.is_success() {
                    let json: serde_json::Value = resp.json().await?;
                    println!("✓ {id} → id={}", json["id"]);
                } else {
                    let text = resp.text().await.unwrap_or_default();
                    eprintln!("✗ {id} → HTTP {status}: {text}");
                }
            }
        }

        Cmd::Publish { post, targets } => {
            let source = load_post(&post)?;
            let ids = resolve_targets(&providers, &targets)?;
            for id in ids {
                let p = providers.get(&id).unwrap();
                let prepared = p.compose(&source)?;
                for w in &prepared.warnings {
                    eprintln!("⚠ [{id}] {w}");
                }
                match p.execute(&prepared).await {
                    Ok(r) => println!(
                        "✓ {id} → {}",
                        r.post_url.as_deref().unwrap_or(&r.platform_id)
                    ),
                    Err(e) => eprintln!("✗ {id} → {e}"),
                }
            }
        }
    }
    Ok(())
}

fn load_post(path: &std::path::Path) -> Result<SourcePost> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading post {}", path.display()))?;
    let post: SourcePost = toml::from_str(&text)?;
    Ok(post)
}
