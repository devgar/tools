//! postkit-core: tipos y traits compartidos entre providers.
//!
//! El modelo de tres etapas:
//!   1. `verify()`      — comprueba credenciales, devuelve info de cuenta.
//!   2. `compose()`     — función pura: (SourcePost, capabilities) -> PreparedPost.
//!                         No hace I/O. Produce un `Vec<Step>` declarativo
//!                         que describe exactamente qué hay que hacer.
//!   3. `execute()`     — ejecuta los `Step`s contra la API de la plataforma.

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

// ─── Input: post lógico, agnóstico de plataforma ─────────────────────────────

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
    /// Overrides por plataforma. Clave = ProviderKind::config_key().
    #[serde(default)]
    pub platforms: HashMap<String, SourcePostOverride>,
}

impl SourcePost {
    /// Devuelve una copia del post con los overrides de la plataforma aplicados.
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

// ─── Output de compose(): plan declarativo ──────────────────────────────────

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
    /// Subir un fichero y guardar la referencia bajo `ref_id` para usar luego.
    UploadMedia { path: PathBuf, alt: Option<String>, ref_id: String },
    /// Crear el post, referenciando medias subidas por `ref_id`.
    /// `facets` es un valor JSON específico de la plataforma — en Bluesky
    /// es el array de facets AT Protocol, en X sería attachments, etc.
    CreatePost {
        text: String,
        #[serde(default)]
        facets: serde_json::Value,
        #[serde(default)]
        media_refs: Vec<String>,
    },
    /// Continuar un hilo (reply al post anterior). Solo providers con
    /// `supports_threads = true` producen este paso.
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
    /// Expiración del access_token en Unix epoch (segundos). None = desconocida.
    pub expires_at: Option<i64>,
}

/// Interfaz mínima para persistir y recuperar tokens renovados fuera de los providers.
/// Implementada por `postkit-store`; los providers solo la llaman, no dependen de la store.
#[async_trait]
pub trait TokenSink: Send + Sync {
    async fn load(&self, account_id: &str) -> anyhow::Result<Option<TokenSet>>;
    async fn save(&self, account_id: &str, tokens: &TokenSet) -> anyhow::Result<()>;
}

// ─── El trait principal ──────────────────────────────────────────────────────

#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn account_id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;

    /// Iter 1: handshake con la API.
    async fn verify(&self) -> anyhow::Result<AccountInfo>;

    /// Iter 2: función pura que adapta un SourcePost a un plan ejecutable.
    /// No debe hacer I/O. Devuelve error si el post no cabe en las
    /// capabilities de la plataforma (ej. texto demasiado largo).
    fn compose(&self, post: &SourcePost) -> anyhow::Result<PreparedPost>;

    /// Iter 3: ejecuta el plan contra la plataforma.
    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult>;
}
