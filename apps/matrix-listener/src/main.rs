//! Matrix listener running outside Windmill.
//!
//! Keeps a long-polling `/sync` against the homeserver and, for every
//! `m.room.message` not sent by the bot itself, launches every script under
//! `f/matrix_bot/subscribers/` asynchronously with `{"event": <event>}`, just
//! like `f/matrix_bot/element_listener` did.
//!
//! Config (environment variables):
//!   WM_BASE_URL          e.g. http://windmill_server:8000
//!   WM_WORKSPACE         e.g. tools
//!   WM_TOKEN             Windmill user token
//!   PING_TRIGGER         optional, defaults to "ping" (empty = disabled)
//!   RUST_LOG             optional, defaults to "info"
//!
//! Each of these, when set and non-empty, takes precedence over the matching
//! Windmill variable:
//!   MATRIX_HOMESERVER    -> f/matrix_bot/homeserver
//!   MATRIX_ACCESS_TOKEN  -> f/matrix_bot/access_token
//!   MATRIX_BOT_NAME      -> f/matrix_bot/bot_name
//!   MATRIX_SINCE_TOKEN   -> f/matrix_bot/since_token (initial seed only)
//!
//! Since token persistence:
//!   MATRIX_SINCE_FILE    when set, the token is read from and saved to this
//!                        file instead of the Windmill variable.

mod events;
mod since;
mod windmill;

use std::{env, time::Duration};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tracing::{error, info, warn};

use crate::{
    events::extract_messages,
    since::SinceStore,
    windmill::{Subscribers, Windmill},
};

const VAR_HOMESERVER: &str = "f/matrix_bot/homeserver";
const VAR_TOKEN: &str = "f/matrix_bot/access_token";
const VAR_BOTNAME: &str = "f/matrix_bot/bot_name";

const SYNC_TIMEOUT_MS: &str = "30000";
const RETRY_DELAY: Duration = Duration::from_secs(5);

/// Only timeline messages matter; presence, account data and ephemeral
/// events are dropped to keep every response small.
const SYNC_FILTER: &str = r#"{"presence":{"types":[]},"account_data":{"types":[]},"room":{"timeline":{"types":["m.room.message"]},"state":{"lazy_load_members":true},"ephemeral":{"types":[]},"account_data":{"types":[]}}}"#;

struct Matrix {
    http: reqwest::Client,
    homeserver: String,
    token: String,
}

impl Matrix {
    async fn sync(&self, since: Option<&str>) -> Result<Value> {
        let mut query = vec![("timeout", SYNC_TIMEOUT_MS), ("filter", SYNC_FILTER)];
        if let Some(since) = since {
            query.push(("since", since));
        }
        Ok(self
            .http
            .get(format!("{}/_matrix/client/v3/sync", self.homeserver))
            .bearer_auth(&self.token)
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn send_text(&self, room_id: &str, body: &str) -> Result<()> {
        let txn = uuid::Uuid::new_v4();
        self.http
            .put(format!(
                "{}/_matrix/client/v3/rooms/{room_id}/send/m.room.message/{txn}",
                self.homeserver
            ))
            .bearer_auth(&self.token)
            .json(&json!({ "msgtype": "m.text", "body": body }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

fn env_req(name: &str) -> Result<String> {
    env_opt(name).with_context(|| format!("missing env var {name}"))
}

fn env_opt(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The env var value when set; otherwise the Windmill variable.
async fn setting(wm: &Windmill, env_name: &str, wm_var: &str) -> Result<String> {
    if let Some(value) = env_opt(env_name) {
        info!(source = env_name, "config from env");
        return Ok(value);
    }
    wm.get_variable(wm_var)
        .await
        .with_context(|| format!("reading {wm_var} from Windmill (or set {env_name})"))
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // The HTTP timeout must be longer than the /sync long-poll.
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()?;

    let wm = Windmill {
        http: http.clone(),
        base: env_req("WM_BASE_URL")?.trim_end_matches('/').to_string(),
        workspace: env_req("WM_WORKSPACE")?,
        token: env_req("WM_TOKEN")?,
    };

    let mx = Matrix {
        http,
        homeserver: setting(&wm, "MATRIX_HOMESERVER", VAR_HOMESERVER)
            .await?
            .trim_end_matches('/')
            .to_string(),
        token: setting(&wm, "MATRIX_ACCESS_TOKEN", VAR_TOKEN).await?,
    };
    let botname = setting(&wm, "MATRIX_BOT_NAME", VAR_BOTNAME).await?;
    let ping = env::var("PING_TRIGGER")
        .unwrap_or_else(|_| "ping".into())
        .trim()
        .to_string();
    let ping = (!ping.is_empty()).then(|| format!("!{ping}"));

    let since_store = match env_opt("MATRIX_SINCE_FILE") {
        Some(path) => SinceStore::File(path.into()),
        None => SinceStore::Windmill(&wm),
    };
    // The env token is only a seed: if it always won, every restart would
    // reprocess the same messages.
    let mut since = since_store.load(env_opt("MATRIX_SINCE_TOKEN")).await;
    let mut subscribers = Subscribers::default();

    info!(homeserver = %mx.homeserver, %botname, resumed = since.is_some(), "listening");

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        let result = tokio::select! {
            r = mx.sync(since.as_deref()) => r,
            _ = &mut shutdown => {
                info!("shutting down");
                return Ok(());
            }
        };

        let response = match result {
            Ok(r) => r,
            Err(e) => {
                error!("sync failed: {e:#}");
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }
        };

        for event in extract_messages(&response, &botname) {
            let room_id = event["room_id"].as_str().unwrap_or_default();
            let body = event["body"].as_str().unwrap_or_default();
            let event_id = event["event_id"].as_str().unwrap_or("?").to_string();

            if let Some(trigger) = &ping
                && body.starts_with(trigger.as_str())
                && let Err(e) = mx.send_text(room_id, "Pong! 🏓").await
            {
                warn!(%event_id, "could not answer ping: {e:#}");
            }

            let args = json!({ "event": event });
            for sub in subscribers.get(&wm).await {
                match wm.run_async(&sub, &args).await {
                    Ok(job) => info!(%event_id, %sub, %job, "dispatched"),
                    Err(e) => error!(%event_id, %sub, "dispatch failed: {e:#}"),
                }
            }
        }

        // Persist after dispatching: if the process dies halfway, those
        // events are retried instead of lost (at-least-once).
        if let Some(next) = response["next_batch"].as_str()
            && since.as_deref() != Some(next)
        {
            if let Err(e) = since_store.save(next).await {
                warn!("could not persist since token: {e:#}");
            }
            since = Some(next.to_string());
        }
    }
}
