mod config;

use anyhow::{Context, Result};
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
    /// Path al config. Admite `~/`.
    #[arg(long, default_value = "~/.config/postkit/config.toml")]
    config: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lista cuentas configuradas.
    Accounts,

    /// Verifica credenciales de una o todas las cuentas.
    Verify { account: Option<String> },

    /// [iter 2] Compone un post y emite el plan JSON sin ejecutarlo.
    Compose {
        /// TOML con el SourcePost.
        post: PathBuf,
        /// Cuentas destino. Si se omite, todas.
        #[arg(long, num_args = 0..)]
        targets: Vec<String>,
    },

    /// [iter 3 lite] Compone y publica inmediatamente (sin scheduling).
    Publish {
        post: PathBuf,
        #[arg(long, num_args = 0..)]
        targets: Vec<String>,
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
            anyhow::bail!("cuenta desconocida: {t}");
        }
    }
    Ok(targets.to_vec())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = expand_tilde(&cli.config);
    let cfg = Config::load(&config_path)
        .with_context(|| format!("cargando config de {}", config_path.display()))?;
    let providers = build_providers(&cfg);

    if providers.is_empty() {
        eprintln!("⚠ no hay cuentas configuradas en {}", config_path.display());
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
                    .ok_or_else(|| anyhow::anyhow!("cuenta desconocida: {id}"))?;
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
        .with_context(|| format!("leyendo post {}", path.display()))?;
    let post: SourcePost = toml::from_str(&text)?;
    Ok(post)
}
