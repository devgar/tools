//! Meta Graph API providers: Facebook Pages and Instagram.
//!
//! Auth: long-lived Page Access Token / User Access Token (Bearer).
//! API base: https://graph.facebook.com/v25.0
//!
//! Facebook Page: binary photo upload + feed post.
//! Instagram: URL-based media containers (requires public URL in MediaRef.url) + two-step publish.
//!
//! Token rotation (opt-in): when `with_app_credentials(app_id, app_secret)` is configured,
//! `ensure_fresh_token()` exchanges the token via `fb_exchange_token` and persists the result
//! in the provided `TokenSink`. The daemon calls this at startup and periodically.

use std::sync::Arc;

use async_trait::async_trait;
use postkit_core::{
    AccountInfo, Capabilities, PreparedPost, Provider, ProviderKind, PublishResult, SourcePost,
    Step, TokenSet, TokenSink,
};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use unicode_segmentation::UnicodeSegmentation;

const GRAPH: &str = "https://graph.facebook.com/v25.0";
const FB_MAX_GRAPHEMES: usize = 63_206;
const FB_MAX_IMAGES: usize = 10;
const IG_MAX_GRAPHEMES: usize = 2_200;
const IG_MAX_IMAGES: usize = 10;
// Assumed validity period after an `fb_exchange_token` exchange (Meta does not return expires_in here).
const META_TOKEN_TTL_SECS: i64 = 60 * 24 * 3600; // 60 days

// ─── Shared helper functions ──────────────────────────────────────────────────

/// Returns `true` if the error originates from an expired or invalid OAuth token.
/// Meta returns HTTP 400/401 with `{"error":{"type":"OAuthException",...}}`.
fn is_oauth_err(e: &anyhow::Error) -> bool {
    if let Some(re) = e.downcast_ref::<reqwest::Error>() {
        if re
            .status()
            .map_or(false, |s| s == reqwest::StatusCode::UNAUTHORIZED || s.as_u16() == 400)
        {
            return true;
        }
    }
    let msg = e.to_string();
    msg.contains("OAuthException") || msg.contains("Invalid OAuth")
}

/// Exchanges a token for a long-lived one via `fb_exchange_token`.
async fn exchange_token(
    http: &Client,
    app_id: &str,
    app_secret: &str,
    current_token: &str,
) -> anyhow::Result<String> {
    #[derive(serde::Deserialize)]
    struct Res {
        access_token: String,
    }
    let res: Res = http
        .get(format!("{GRAPH}/oauth/access_token"))
        .query(&[
            ("grant_type", "fb_exchange_token"),
            ("client_id", app_id),
            ("client_secret", app_secret),
            ("fb_exchange_token", current_token),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(res.access_token)
}

// ─── Facebook Page ────────────────────────────────────────────────────────────

pub struct FacebookPage {
    account_id: String,
    page_id: String,
    token: Arc<RwLock<String>>,
    /// (app_id, app_secret) required for user token rotation.
    app_credentials: Option<(String, String)>,
    token_sink: Option<Arc<dyn TokenSink>>,
    http: Client,
}

impl FacebookPage {
    pub fn new(account_id: String, page_id: String, page_access_token: String) -> Self {
        Self {
            account_id,
            page_id,
            token: Arc::new(RwLock::new(page_access_token)),
            app_credentials: None,
            token_sink: None,
            http: Client::new(),
        }
    }

    pub fn with_app_credentials(mut self, app_id: String, app_secret: String) -> Self {
        self.app_credentials = Some((app_id, app_secret));
        self
    }

    pub fn with_token_sink(mut self, sink: Arc<dyn TokenSink>) -> Self {
        self.token_sink = Some(sink);
        self
    }

    /// Exchanges the token for a long-lived one and persists the result.
    /// Returns `true` if the token was refreshed; `false` if no `app_credentials` are set.
    pub async fn ensure_fresh_token(&self) -> anyhow::Result<bool> {
        let Some((ref app_id, ref app_secret)) = self.app_credentials else {
            return Ok(false);
        };
        let current = self.token.read().await.clone();
        let new_token = exchange_token(&self.http, app_id, app_secret, &current).await?;
        *self.token.write().await = new_token.clone();
        if let Some(ref sink) = self.token_sink {
            let _ = sink
                .save(
                    &self.account_id,
                    &TokenSet {
                        access_token: new_token,
                        refresh_token: None,
                        expires_at: Some(chrono::Utc::now().timestamp() + META_TOKEN_TTL_SECS),
                    },
                )
                .await;
        }
        Ok(true)
    }

    async fn upload_photo(&self, bytes: Vec<u8>, alt: Option<&str>) -> anyhow::Result<String> {
        let token = self.token.read().await.clone();
        let url = format!("{GRAPH}/{}/photos", self.page_id);
        let part = reqwest::multipart::Part::bytes(bytes).file_name("photo.jpg");
        let mut form = reqwest::multipart::Form::new()
            .text("published", "false")
            .text("access_token", token)
            .part("source", part);
        if let Some(text) = alt {
            form = form.text("alt_text_custom", text.to_string());
        }
        let res: Value = self
            .http
            .post(&url)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        res["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Facebook upload: missing id in response"))
    }

    async fn execute_inner(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        let token = self.token.read().await.clone();
        let mut media_fbids: Vec<String> = Vec::new();
        let mut post_text = String::new();
        let mut post_media_refs: Vec<String> = Vec::new();

        for step in &prepared.steps {
            match step {
                Step::UploadMedia { path, alt, .. } => {
                    let bytes = tokio::fs::read(path).await?;
                    let id = self.upload_photo(bytes, alt.as_deref()).await?;
                    media_fbids.push(id);
                }
                Step::CreatePost { text, media_refs, .. } => {
                    post_text = text.clone();
                    post_media_refs = media_refs.clone();
                }
                Step::ThreadContinue { .. } => {}
            }
        }

        let mut body = json!({ "message": post_text, "access_token": token });
        if !post_media_refs.is_empty() {
            let attached: Vec<Value> = media_fbids
                .iter()
                .map(|id| json!({ "media_fbid": id }))
                .collect();
            body["attached_media"] = json!(attached);
        }

        let res: Value = self
            .http
            .post(format!("{GRAPH}/{}/feed", self.page_id))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let id = res["id"].as_str().unwrap_or("").to_string();
        let parts: Vec<&str> = id.splitn(2, '_').collect();
        let post_url = if parts.len() == 2 {
            Some(format!("https://www.facebook.com/{}/posts/{}", parts[0], parts[1]))
        } else {
            None
        };
        Ok(PublishResult { post_url, platform_id: id, raw: res })
    }
}

#[async_trait]
impl Provider for FacebookPage {
    fn kind(&self) -> ProviderKind { ProviderKind::MetaPage }
    fn account_id(&self) -> &str { &self.account_id }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_text_graphemes: FB_MAX_GRAPHEMES,
            max_media: FB_MAX_IMAGES,
            supports_threads: false,
            supports_alt_text: true,
        }
    }

    async fn verify(&self) -> anyhow::Result<AccountInfo> {
        let token = self.token.read().await.clone();
        let body: serde_json::Value = self
            .http
            .get(format!("{GRAPH}/{}", self.page_id))
            .query(&[("fields", "name,username"), ("access_token", &token)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(err) = body.get("error") {
            anyhow::bail!("Meta API error: {}", err);
        }
        let name = body["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("unexpected response: {body}"))?
            .to_string();
        let handle = body["username"]
            .as_str()
            .unwrap_or(&self.page_id)
            .to_string();
        Ok(AccountInfo {
            id: self.account_id.clone(),
            provider: ProviderKind::MetaPage,
            handle,
            display_name: Some(name),
        })
    }

    fn compose(&self, post: &SourcePost) -> anyhow::Result<PreparedPost> {
        let mut text = post.text.clone();
        if text.graphemes(true).count() > FB_MAX_GRAPHEMES {
            anyhow::bail!(
                "Facebook: text too long ({} graphemes, max {})",
                text.graphemes(true).count(),
                FB_MAX_GRAPHEMES
            );
        }
        if post.media.len() > FB_MAX_IMAGES {
            anyhow::bail!(
                "Facebook: maximum {} images, {} provided",
                FB_MAX_IMAGES,
                post.media.len()
            );
        }
        if !post.hashtags.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            let tags: Vec<String> = post.hashtags.iter().map(|t| format!("#{t}")).collect();
            text.push_str(&tags.join(" "));
        }
        let mut steps = Vec::new();
        let mut warnings = Vec::new();
        let mut media_refs = Vec::new();
        for (i, m) in post.media.iter().enumerate() {
            let ref_id = format!("img{i}");
            if m.alt.is_none() {
                warnings.push(format!("Image {i} missing alt text"));
            }
            steps.push(Step::UploadMedia {
                path: m.path.clone(),
                alt: m.alt.clone(),
                ref_id: ref_id.clone(),
            });
            media_refs.push(ref_id);
        }
        steps.push(Step::CreatePost { text, facets: Value::Null, media_refs });
        Ok(PreparedPost {
            account_id: self.account_id.clone(),
            provider: ProviderKind::MetaPage,
            steps,
            warnings,
        })
    }

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        match self.execute_inner(prepared).await {
            Err(e) if is_oauth_err(&e) && self.app_credentials.is_some() => {
                match self.ensure_fresh_token().await {
                    Ok(true) => self.execute_inner(prepared).await,
                    _ => Err(e),
                }
            }
            other => other,
        }
    }
}

// ─── Instagram ────────────────────────────────────────────────────────────────

pub struct Instagram {
    account_id: String,
    ig_user_id: String,
    token: Arc<RwLock<String>>,
    /// (app_id, app_secret) required for user token rotation.
    app_credentials: Option<(String, String)>,
    token_sink: Option<Arc<dyn TokenSink>>,
    http: Client,
}

impl Instagram {
    pub fn new(account_id: String, ig_user_id: String, access_token: String) -> Self {
        Self {
            account_id,
            ig_user_id,
            token: Arc::new(RwLock::new(access_token)),
            app_credentials: None,
            token_sink: None,
            http: Client::new(),
        }
    }

    pub fn with_app_credentials(mut self, app_id: String, app_secret: String) -> Self {
        self.app_credentials = Some((app_id, app_secret));
        self
    }

    pub fn with_token_sink(mut self, sink: Arc<dyn TokenSink>) -> Self {
        self.token_sink = Some(sink);
        self
    }

    /// Exchanges the token for a long-lived one and persists the result.
    /// Returns `true` if the token was refreshed; `false` if no `app_credentials` are set.
    pub async fn ensure_fresh_token(&self) -> anyhow::Result<bool> {
        let Some((ref app_id, ref app_secret)) = self.app_credentials else {
            return Ok(false);
        };
        let current = self.token.read().await.clone();
        let new_token = exchange_token(&self.http, app_id, app_secret, &current).await?;
        *self.token.write().await = new_token.clone();
        if let Some(ref sink) = self.token_sink {
            let _ = sink
                .save(
                    &self.account_id,
                    &TokenSet {
                        access_token: new_token,
                        refresh_token: None,
                        expires_at: Some(chrono::Utc::now().timestamp() + META_TOKEN_TTL_SECS),
                    },
                )
                .await;
        }
        Ok(true)
    }

    async fn create_container(
        &self,
        image_url: &str,
        caption: Option<&str>,
        carousel_item: bool,
    ) -> anyhow::Result<String> {
        let token = self.token.read().await.clone();
        let mut params = vec![("image_url", image_url.to_string()), ("access_token", token)];
        if carousel_item {
            params.push(("is_carousel_item", "true".to_string()));
        } else if let Some(cap) = caption {
            params.push(("caption", cap.to_string()));
        }
        let res: Value = self
            .http
            .post(format!("{GRAPH}/{}/media", self.ig_user_id))
            .form(&params)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        res["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Instagram create_container: missing id"))
    }

    async fn create_carousel_container(
        &self,
        children: &[String],
        caption: &str,
    ) -> anyhow::Result<String> {
        let token = self.token.read().await.clone();
        let children_str = children.join(",");
        let res: Value = self
            .http
            .post(format!("{GRAPH}/{}/media", self.ig_user_id))
            .form(&[
                ("media_type", "CAROUSEL"),
                ("children", &children_str),
                ("caption", caption),
                ("access_token", &token),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        res["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Instagram create_carousel: missing id"))
    }

    async fn publish_container(&self, creation_id: &str) -> anyhow::Result<String> {
        let token = self.token.read().await.clone();
        let res: Value = self
            .http
            .post(format!("{GRAPH}/{}/media_publish", self.ig_user_id))
            .form(&[("creation_id", creation_id), ("access_token", &token)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        res["id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("Instagram media_publish: missing id"))
    }

    async fn execute_inner(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        let Step::CreatePost { text, facets, media_refs } = prepared
            .steps
            .iter()
            .find(|s| matches!(s, Step::CreatePost { .. }))
            .ok_or_else(|| anyhow::anyhow!("Instagram execute: no CreatePost step"))?
        else {
            unreachable!()
        };

        let url_map: std::collections::HashMap<String, String> = facets
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|e| {
                let ref_id = e["ref_id"].as_str()?.to_string();
                let url = e["url"].as_str()?.to_string();
                Some((ref_id, url))
            })
            .collect();

        let media_id = if media_refs.len() == 1 {
            let url = url_map
                .get(&media_refs[0])
                .ok_or_else(|| anyhow::anyhow!("Instagram: URL not found for img0"))?;
            let creation_id = self.create_container(url, Some(text), false).await?;
            self.publish_container(&creation_id).await?
        } else {
            let mut children = Vec::new();
            for ref_id in media_refs {
                let url = url_map
                    .get(ref_id)
                    .ok_or_else(|| anyhow::anyhow!("Instagram: URL not found for {ref_id}"))?;
                let id = self.create_container(url, None, true).await?;
                children.push(id);
            }
            let carousel_id = self.create_carousel_container(&children, text).await?;
            self.publish_container(&carousel_id).await?
        };

        let token = self.token.read().await.clone();
        let post_url = async {
            let body: Value = self
                .http
                .get(format!("{GRAPH}/{media_id}"))
                .query(&[("fields", "permalink"), ("access_token", token.as_str())])
                .send()
                .await
                .ok()?
                .json()
                .await
                .ok()?;
            body["permalink"].as_str().map(String::from)
        }
        .await;

        Ok(PublishResult {
            post_url,
            platform_id: media_id.clone(),
            raw: json!({ "id": media_id }),
        })
    }
}

#[async_trait]
impl Provider for Instagram {
    fn kind(&self) -> ProviderKind { ProviderKind::MetaInstagram }
    fn account_id(&self) -> &str { &self.account_id }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_text_graphemes: IG_MAX_GRAPHEMES,
            max_media: IG_MAX_IMAGES,
            supports_threads: false,
            supports_alt_text: false,
        }
    }

    async fn verify(&self) -> anyhow::Result<AccountInfo> {
        let token = self.token.read().await.clone();
        let body: serde_json::Value = self
            .http
            .get(format!("{GRAPH}/{}", self.ig_user_id))
            .query(&[("fields", "name,username"), ("access_token", &token)])
            .send()
            .await?
            .json()
            .await?;
        if let Some(err) = body.get("error") {
            anyhow::bail!("Meta API error: {}", err);
        }
        let username = body["username"]
            .as_str()
            .or_else(|| body["name"].as_str())
            .ok_or_else(|| anyhow::anyhow!("unexpected response: {body}"))?
            .to_string();
        let display_name = body["name"].as_str().map(str::to_string);
        Ok(AccountInfo {
            id: self.account_id.clone(),
            provider: ProviderKind::MetaInstagram,
            handle: username,
            display_name,
        })
    }

    fn compose(&self, post: &SourcePost) -> anyhow::Result<PreparedPost> {
        let mut text = post.text.clone();
        if text.graphemes(true).count() > IG_MAX_GRAPHEMES {
            anyhow::bail!(
                "Instagram: caption too long ({} graphemes, max {})",
                text.graphemes(true).count(),
                IG_MAX_GRAPHEMES
            );
        }
        if post.media.is_empty() {
            anyhow::bail!("Instagram: at least one image is required");
        }
        if post.media.len() > IG_MAX_IMAGES {
            anyhow::bail!(
                "Instagram: maximum {} images, {} provided",
                IG_MAX_IMAGES,
                post.media.len()
            );
        }
        for (i, m) in post.media.iter().enumerate() {
            if m.url.is_none() {
                anyhow::bail!("Instagram: image {i} has no public URL (field `url` required)");
            }
        }
        if !post.hashtags.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            let tags: Vec<String> = post.hashtags.iter().map(|t| format!("#{t}")).collect();
            text.push_str(&tags.join(" "));
        }
        let mut warnings = Vec::new();
        let mut media_refs: Vec<String> = Vec::new();
        let mut url_entries: Vec<Value> = Vec::new();
        for (i, m) in post.media.iter().enumerate() {
            let ref_id = format!("img{i}");
            if m.alt.is_none() {
                warnings.push(format!("Image {i} missing alt text"));
            }
            url_entries.push(json!({ "ref_id": ref_id, "url": m.url }));
            media_refs.push(ref_id);
        }
        let steps = vec![Step::CreatePost { text, facets: json!(url_entries), media_refs }];
        Ok(PreparedPost {
            account_id: self.account_id.clone(),
            provider: ProviderKind::MetaInstagram,
            steps,
            warnings,
        })
    }

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        match self.execute_inner(prepared).await {
            Err(e) if is_oauth_err(&e) && self.app_credentials.is_some() => {
                match self.ensure_fresh_token().await {
                    Ok(true) => self.execute_inner(prepared).await,
                    _ => Err(e),
                }
            }
            other => other,
        }
    }
}

// ─── MetaProvider trait ───────────────────────────────────────────────────────

#[async_trait]
pub trait MetaProvider: postkit_core::Provider + 'static {
    fn with_app_credentials(self, app_id: String, app_secret: String) -> Self;
    async fn ensure_fresh_token(&self) -> anyhow::Result<bool>;
}

#[async_trait]
impl MetaProvider for FacebookPage {
    fn with_app_credentials(self, app_id: String, app_secret: String) -> Self {
        FacebookPage::with_app_credentials(self, app_id, app_secret)
    }
    async fn ensure_fresh_token(&self) -> anyhow::Result<bool> {
        FacebookPage::ensure_fresh_token(self).await
    }
}

#[async_trait]
impl MetaProvider for Instagram {
    fn with_app_credentials(self, app_id: String, app_secret: String) -> Self {
        Instagram::with_app_credentials(self, app_id, app_secret)
    }
    async fn ensure_fresh_token(&self) -> anyhow::Result<bool> {
        Instagram::ensure_fresh_token(self).await
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use postkit_core::{MediaRef, SourcePost};

    use super::*;

    fn fb() -> FacebookPage { FacebookPage::new("test".into(), "123".into(), "TOKEN".into()) }

    fn ig() -> Instagram { Instagram::new("test".into(), "456".into(), "TOKEN".into()) }

    fn src(text: &str) -> SourcePost {
        SourcePost {
            text: text.into(),
            media: vec![],
            hashtags: vec![],
            platforms: Default::default(),
        }
    }

    fn media_with_url(url: &str) -> MediaRef {
        MediaRef { path: PathBuf::from("img.jpg"), alt: None, url: Some(url.into()) }
    }

    fn media_without_url() -> MediaRef {
        MediaRef { path: PathBuf::from("img.jpg"), alt: None, url: None }
    }

    // ── FacebookPage ──────────────────────────────────────────────────────────

    #[test]
    fn fb_compose_basic_post() {
        let result = fb().compose(&src("Hello")).unwrap();
        assert!(result.warnings.is_empty());
        match &result.steps[0] {
            Step::CreatePost { text, media_refs, .. } => {
                assert_eq!(text, "Hello");
                assert!(media_refs.is_empty());
            }
            _ => panic!("expected CreatePost"),
        }
    }

    #[test]
    fn fb_compose_appends_hashtags() {
        let post = SourcePost {
            text: "Hi".into(),
            hashtags: vec!["rust".into()],
            media: vec![],
            platforms: Default::default(),
        };
        let result = fb().compose(&post).unwrap();
        let Step::CreatePost { text, .. } = &result.steps[0] else { panic!() };
        assert_eq!(text, "Hi\n\n#rust");
    }

    #[test]
    fn fb_compose_allows_exactly_limit() {
        assert!(fb().compose(&src(&"a".repeat(FB_MAX_GRAPHEMES))).is_ok());
    }

    #[test]
    fn fb_compose_rejects_over_limit() {
        assert!(fb()
            .compose(&src(&"a".repeat(FB_MAX_GRAPHEMES + 1)))
            .is_err());
    }

    #[test]
    fn fb_compose_counts_emoji_as_one_grapheme() {
        let text = format!("{}{}", "a".repeat(FB_MAX_GRAPHEMES - 1), "🦀");
        assert!(fb().compose(&src(&text)).is_ok());
    }

    #[test]
    fn fb_compose_rejects_more_than_max_images() {
        let media = (0..=FB_MAX_IMAGES)
            .map(|i| MediaRef { path: PathBuf::from(format!("img{i}.png")), alt: None, url: None })
            .collect();
        let post = SourcePost {
            text: "test".into(),
            media,
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(fb().compose(&post).is_err());
    }

    #[test]
    fn fb_compose_warns_on_missing_alt() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![MediaRef { path: PathBuf::from("img.png"), alt: None, url: None }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(!fb().compose(&post).unwrap().warnings.is_empty());
    }

    #[test]
    fn fb_compose_no_warning_with_alt() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![MediaRef {
                path: PathBuf::from("img.png"),
                alt: Some("desc".into()),
                url: None,
            }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(fb().compose(&post).unwrap().warnings.is_empty());
    }

    #[test]
    fn fb_compose_upload_step_carries_alt() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![MediaRef {
                path: PathBuf::from("img.png"),
                alt: Some("a crab".into()),
                url: None,
            }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        let result = fb().compose(&post).unwrap();
        match &result.steps[0] {
            Step::UploadMedia { alt, .. } => assert_eq!(alt.as_deref(), Some("a crab")),
            _ => panic!("expected UploadMedia"),
        }
    }

    #[test]
    fn fb_compose_generates_upload_steps_for_media() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![
                MediaRef { path: PathBuf::from("a.png"), alt: Some("A".into()), url: None },
                MediaRef { path: PathBuf::from("b.png"), alt: Some("B".into()), url: None },
            ],
            hashtags: vec![],
            platforms: Default::default(),
        };
        let result = fb().compose(&post).unwrap();
        assert_eq!(result.steps.len(), 3);
        assert!(matches!(result.steps[0], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[1], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[2], Step::CreatePost { .. }));
    }

    // ── Instagram ─────────────────────────────────────────────────────────────

    #[test]
    fn ig_compose_basic_post_with_image() {
        let post = SourcePost {
            text: "Hello".into(),
            media: vec![MediaRef {
                path: PathBuf::from("img.jpg"),
                alt: Some("photo".into()),
                url: Some("https://example.com/img.jpg".into()),
            }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        let result = ig().compose(&post).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(result.steps.len(), 1);
        let Step::CreatePost { text, media_refs, facets } = &result.steps[0] else { panic!() };
        assert_eq!(text, "Hello");
        assert_eq!(media_refs.len(), 1);
        assert!(facets.is_array());
    }

    #[test]
    fn ig_compose_appends_hashtags() {
        let post = SourcePost {
            text: "Hi".into(),
            hashtags: vec!["rust".into()],
            media: vec![media_with_url("https://example.com/img.jpg")],
            platforms: Default::default(),
        };
        let result = ig().compose(&post).unwrap();
        let Step::CreatePost { text, .. } = &result.steps[0] else { panic!() };
        assert_eq!(text, "Hi\n\n#rust");
    }

    #[test]
    fn ig_compose_allows_exactly_limit() {
        let post = SourcePost {
            text: "a".repeat(IG_MAX_GRAPHEMES),
            media: vec![media_with_url("https://example.com/img.jpg")],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(ig().compose(&post).is_ok());
    }

    #[test]
    fn ig_compose_rejects_over_limit() {
        let post = SourcePost {
            text: "a".repeat(IG_MAX_GRAPHEMES + 1),
            media: vec![media_with_url("https://example.com/img.jpg")],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(ig().compose(&post).is_err());
    }

    #[test]
    fn ig_compose_counts_emoji_as_one_grapheme() {
        let text = format!("{}{}", "a".repeat(IG_MAX_GRAPHEMES - 1), "🦀");
        let post = SourcePost {
            text,
            media: vec![media_with_url("https://example.com/img.jpg")],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(ig().compose(&post).is_ok());
    }

    #[test]
    fn ig_compose_rejects_without_media() {
        assert!(ig().compose(&src("Hello")).is_err());
    }

    #[test]
    fn ig_compose_rejects_more_than_max_images() {
        let media = (0..=IG_MAX_IMAGES)
            .map(|i| MediaRef {
                path: PathBuf::from(format!("img{i}.png")),
                alt: None,
                url: Some(format!("https://example.com/img{i}.png")),
            })
            .collect();
        let post = SourcePost {
            text: "test".into(),
            media,
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(ig().compose(&post).is_err());
    }

    #[test]
    fn ig_compose_rejects_media_without_url() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![media_without_url()],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(ig().compose(&post).is_err());
    }

    #[test]
    fn ig_compose_warns_on_missing_alt() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![media_with_url("https://example.com/img.jpg")],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(!ig().compose(&post).unwrap().warnings.is_empty());
    }

    #[test]
    fn ig_compose_no_warning_with_alt() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![MediaRef {
                path: PathBuf::from("img.jpg"),
                alt: Some("desc".into()),
                url: Some("https://example.com/img.jpg".into()),
            }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(ig().compose(&post).unwrap().warnings.is_empty());
    }

    #[test]
    fn ig_compose_carousel_has_multiple_refs() {
        let post = SourcePost {
            text: "test".into(),
            media: vec![
                media_with_url("https://example.com/a.jpg"),
                media_with_url("https://example.com/b.jpg"),
            ],
            hashtags: vec![],
            platforms: Default::default(),
        };
        let result = ig().compose(&post).unwrap();
        let Step::CreatePost { media_refs, facets, .. } = &result.steps[0] else { panic!() };
        assert_eq!(media_refs.len(), 2);
        assert_eq!(facets.as_array().unwrap().len(), 2);
    }

    // ── Snapshot tests ────────────────────────────────────────────────────────

    #[test]
    fn snap_fb_post_with_media_and_alt() {
        let post = SourcePost {
            text: "Hello Facebook".into(),
            media: vec![
                MediaRef { path: PathBuf::from("a.jpg"), alt: Some("image A".into()), url: None },
                MediaRef { path: PathBuf::from("b.jpg"), alt: Some("image B".into()), url: None },
            ],
            hashtags: vec!["rust".into()],
            platforms: Default::default(),
        };
        let result = fb().compose(&post).unwrap();
        insta::assert_json_snapshot!(&result.steps);
    }

    #[test]
    fn snap_ig_carousel() {
        let post = SourcePost {
            text: "Hello Instagram".into(),
            media: vec![
                media_with_url("https://example.com/a.jpg"),
                media_with_url("https://example.com/b.jpg"),
            ],
            hashtags: vec!["rust".into()],
            platforms: Default::default(),
        };
        let result = ig().compose(&post).unwrap();
        insta::assert_json_snapshot!(&result.steps);
    }
}
