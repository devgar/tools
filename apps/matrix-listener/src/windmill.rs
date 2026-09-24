//! Minimal Windmill REST client (endpoints checked against the v1.817 OpenAPI).

use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};
use tracing::{info, warn};

pub const SUBSCRIBERS_PREFIX: &str = "f/matrix_bot/subscribers/";
const SUBSCRIBERS_TTL: Duration = Duration::from_secs(60);

pub struct Windmill {
    pub http: reqwest::Client,
    pub base: String,
    pub workspace: String,
    pub token: String,
}

impl Windmill {
    fn url(&self, path: &str) -> String {
        format!("{}/api/w/{}/{}", self.base, self.workspace, path)
    }

    /// `getVariableValue`: returns the (decrypted) value as a JSON string.
    pub async fn get_variable(&self, path: &str) -> Result<String> {
        let value: Value = self
            .http
            .get(self.url(&format!("variables/get_value/{path}")))
            .bearer_auth(&self.token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// `updateVariable`: the server encrypts the plain value itself when the
    /// variable is secret (`already_encrypted` defaults to false).
    pub async fn set_variable(&self, path: &str, value: &str) -> Result<()> {
        self.http
            .post(self.url(&format!("variables/update/{path}")))
            .bearer_auth(&self.token)
            .json(&json!({ "value": value }))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn list_subscribers(&self) -> Result<Vec<String>> {
        let scripts: Vec<Value> = self
            .http
            .get(self.url("scripts/list"))
            .bearer_auth(&self.token)
            .query(&[("path_start", SUBSCRIBERS_PREFIX), ("per_page", "100")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(scripts
            .iter()
            .filter_map(|s| s["path"].as_str())
            .filter(|p| p.starts_with(SUBSCRIBERS_PREFIX))
            .map(String::from)
            .collect())
    }

    /// `runScriptByPath`: the body is the script args; answers 201 with the
    /// job uuid as `text/plain`. Does not wait for the job.
    pub async fn run_async(&self, path: &str, args: &Value) -> Result<String> {
        let job = self
            .http
            .post(self.url(&format!("jobs/run/p/{path}")))
            .bearer_auth(&self.token)
            .json(args)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(job.trim().trim_matches('"').to_string())
    }
}

#[derive(Default)]
pub struct Subscribers {
    paths: Vec<String>,
    fetched: Option<Instant>,
}

impl Subscribers {
    /// Refreshes the list at most once per TTL; if Windmill fails, keeps
    /// using the last known list.
    pub async fn get(&mut self, wm: &Windmill) -> Vec<String> {
        let stale = self.fetched.is_none_or(|t| t.elapsed() > SUBSCRIBERS_TTL);
        if stale {
            match wm.list_subscribers().await {
                Ok(paths) => {
                    if paths != self.paths {
                        info!(?paths, "subscribers updated");
                    }
                    self.paths = paths;
                    self.fetched = Some(Instant::now());
                }
                Err(e) => warn!("could not refresh subscribers, using cached list: {e:#}"),
            }
        }
        self.paths.clone()
    }
}
