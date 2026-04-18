//! postkit-core: tipos y traits compartidos entre providers.
//!
//! El modelo de tres etapas:
//!   1. `verify()`      — comprueba credenciales, devuelve info de cuenta.
//!   2. `compose()`     — función pura: (SourcePost, capabilities) -> PreparedPost.
//!                         No hace I/O. Produce un `Vec<Step>` declarativo
//!                         que describe exactamente qué hay que hacer.
//!   3. `execute()`     — ejecuta los `Step`s contra la API de la plataforma.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePost {
    pub text: String,
    #[serde(default)]
    pub media: Vec<MediaRef>,
    #[serde(default)]
    pub hashtags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaRef {
    pub path: PathBuf,
    #[serde(default)]
    pub alt: Option<String>,
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
    UploadMedia {
        path: PathBuf,
        alt: Option<String>,
        ref_id: String,
    },
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResult {
    pub post_url: Option<String>,
    pub platform_id: String,
    pub raw: serde_json::Value,
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
