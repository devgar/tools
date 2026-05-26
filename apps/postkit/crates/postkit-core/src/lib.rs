//! postkit-core: shared types and traits across providers.
//!
//! Three-stage model:
//!   1. `verify()`      — validates credentials, returns account info.
//!   2. `compose()`     — pure function: (SourcePost, capabilities) -> PreparedPost.
//!                         No I/O. Produces a declarative `Vec<Step>` that
//!                         describes exactly what needs to happen.
//!   3. `execute()`     — executes the `Step`s against the platform API.

use std::{collections::HashMap, path::PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Bluesky,
    X,
    MetaPage,
    MetaInstagram,
    YouTube,
    TikTok,
}

impl ProviderKind {
    pub fn config_key(&self) -> &'static str {
        match self {
            ProviderKind::Bluesky => "bluesky",
            ProviderKind::X => "x",
            ProviderKind::MetaPage => "meta_page",
            ProviderKind::MetaInstagram => "meta_instagram",
            ProviderKind::YouTube => "youtube",
            ProviderKind::TikTok => "tiktok",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub max_text_graphemes: usize,
    pub max_media: usize,
    pub supports_threads: bool,
    pub supports_alt_text: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub id: String,
    pub provider: ProviderKind,
    pub handle: String,
    pub display_name: Option<String>,
}

// ─── Input: logical post, platform-agnostic ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourcePostOverride {
    pub text: Option<String>,
    pub media: Option<Vec<MediaRef>>,
    pub hashtags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourcePost {
    pub text: String,
    #[serde(default)]
    pub media: Vec<MediaRef>,
    #[serde(default)]
    pub hashtags: Vec<String>,
    /// Per-platform overrides. Key = ProviderKind::config_key().
    #[serde(default)]
    pub platforms: HashMap<String, SourcePostOverride>,
}

impl SourcePost {
    pub fn resolve(&self, kind: ProviderKind) -> SourcePost {
        let ov = self.platforms.get(kind.config_key());
        SourcePost {
            text: ov
                .and_then(|o| o.text.clone())
                .unwrap_or_else(|| self.text.clone()),
            media: ov
                .and_then(|o| o.media.clone())
                .unwrap_or_else(|| self.media.clone()),
            hashtags: ov
                .and_then(|o| o.hashtags.clone())
                .unwrap_or_else(|| self.hashtags.clone()),
            platforms: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub path: PathBuf,
    #[serde(default)]
    pub alt: Option<String>,
    /// Public URL for platforms that require URL-based media uploads (e.g. Instagram).
    #[serde(default)]
    pub url: Option<String>,
}

// ─── Output of compose(): declarative plan ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedPost {
    pub account_id: String,
    pub provider: ProviderKind,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// Upload a file and store the reference under `ref_id` for later use.
    UploadMedia { path: PathBuf, alt: Option<String>, ref_id: String },
    /// Create the post, referencing uploaded media by `ref_id`.
    /// `facets` is a platform-specific JSON value — in Bluesky it is the
    /// AT Protocol facets array; in X it would be attachments, etc.
    CreatePost {
        text: String,
        #[serde(default)]
        facets: serde_json::Value,
        #[serde(default)]
        media_refs: Vec<String>,
    },
    /// Continue a thread (reply to the previous post). Only providers with
    /// `supports_threads = true` produce this step.
    ThreadContinue {
        text: String,
        #[serde(default)]
        facets: serde_json::Value,
        #[serde(default)]
        media_refs: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResult {
    pub post_url: Option<String>,
    pub platform_id: String,
    pub raw: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_returns_base_when_no_override() {
        let post = SourcePost {
            text: "hello".into(),
            hashtags: vec!["rust".into()],
            ..Default::default()
        };
        let resolved = post.resolve(ProviderKind::Bluesky);
        assert_eq!(resolved.text, "hello");
        assert_eq!(resolved.hashtags, vec!["rust"]);
        assert!(resolved.platforms.is_empty());
    }

    #[test]
    fn resolve_applies_text_override() {
        let mut platforms = HashMap::new();
        platforms.insert(
            "x".into(),
            SourcePostOverride { text: Some("short for X".into()), media: None, hashtags: None },
        );
        let post = SourcePost { text: "long default text".into(), platforms, ..Default::default() };
        let resolved = post.resolve(ProviderKind::X);
        assert_eq!(resolved.text, "short for X");
        assert!(resolved.platforms.is_empty());
    }

    #[test]
    fn resolve_keeps_base_text_for_other_platforms() {
        let mut platforms = HashMap::new();
        platforms.insert(
            "x".into(),
            SourcePostOverride { text: Some("short for X".into()), media: None, hashtags: None },
        );
        let post = SourcePost { text: "default text".into(), platforms, ..Default::default() };
        let resolved = post.resolve(ProviderKind::Bluesky);
        assert_eq!(resolved.text, "default text");
    }
}

// ─── Token persistence ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Expiry of the access_token as Unix epoch (seconds). None = unknown.
    pub expires_at: Option<i64>,
}

/// Minimal interface for persisting and retrieving refreshed tokens outside providers.
/// Implemented by `postkit-store`; providers only call it and do not depend on the store.
#[async_trait]
pub trait TokenSink: Send + Sync {
    async fn load(&self, account_id: &str) -> anyhow::Result<Option<TokenSet>>;
    async fn save(&self, account_id: &str, tokens: &TokenSet) -> anyhow::Result<()>;
}

// ─── Main trait ──────────────────────────────────────────────────────────────

#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn account_id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;

    async fn verify(&self) -> anyhow::Result<AccountInfo>;

    /// Pure function that adapts a SourcePost into an executable plan.
    /// Must not perform I/O. Returns an error if the post exceeds platform
    /// capabilities (e.g. text too long).
    fn compose(&self, post: &SourcePost) -> anyhow::Result<PreparedPost>;

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult>;
}
