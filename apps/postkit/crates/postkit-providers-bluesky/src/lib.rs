//! Bluesky provider, usando XRPC directo sobre reqwest.
//!
//! Notas sobre AT Protocol:
//! - Auth: `createSession` con handle + app password, devuelve JWTs.
//! - Texto: max 300 grafemas (no chars, no bytes).
//! - Facets: para que links/tags sean clicables hay que anotar byte offsets
//!   en un array de facets aparte del texto.
//! - Media: subir con `uploadBlob`, luego embeder el objeto blob en el record.

use async_trait::async_trait;
use chrono::Utc;
use postkit_core::*;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use unicode_segmentation::UnicodeSegmentation;

const PDS: &str = "https://bsky.social";
const MAX_GRAPHEMES: usize = 300;
const MAX_IMAGES: usize = 4;

#[derive(Debug, Deserialize, Clone)]
struct Session {
    #[serde(rename = "accessJwt")]
    access_jwt: String,
    #[serde(rename = "refreshJwt")]
    #[allow(dead_code)]
    refresh_jwt: String,
    did: String,
    handle: String,
}

pub struct Bluesky {
    account_id: String,
    handle: String,
    app_password: String,
    http: Client,
    session: Arc<RwLock<Option<Session>>>,
}

impl Bluesky {
    pub fn new(account_id: String, handle: String, app_password: String) -> Self {
        Self {
            account_id,
            handle,
            app_password,
            http: Client::new(),
            session: Arc::new(RwLock::new(None)),
        }
    }

    async fn ensure_session(&self) -> anyhow::Result<Session> {
        if let Some(s) = self.session.read().await.clone() {
            return Ok(s);
        }
        let res: Session = self
            .http
            .post(format!("{PDS}/xrpc/com.atproto.server.createSession"))
            .json(&json!({
                "identifier": self.handle,
                "password": self.app_password,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        *self.session.write().await = Some(res.clone());
        Ok(res)
    }

    async fn upload_blob(&self, bytes: Vec<u8>, mime: &str) -> anyhow::Result<Value> {
        let s = self.ensure_session().await?;
        let res: Value = self
            .http
            .post(format!("{PDS}/xrpc/com.atproto.repo.uploadBlob"))
            .bearer_auth(&s.access_jwt)
            .header("Content-Type", mime)
            .body(bytes)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(res["blob"].clone())
    }
}

/// Detecta URLs y hashtags, devuelve el array de facets AT con byte offsets.
fn detect_facets(text: &str) -> Value {
    let mut facets: Vec<Value> = Vec::new();

    let url_re = regex::Regex::new(r"https?://[^\s]+").unwrap();
    for m in url_re.find_iter(text) {
        facets.push(json!({
            "index": { "byteStart": m.start(), "byteEnd": m.end() },
            "features": [{ "$type": "app.bsky.richtext.facet#link", "uri": m.as_str() }]
        }));
    }

    // (^|\s)#tag — el '#' está en word.start()-1
    let tag_re = regex::Regex::new(r"(?:^|\s)#(\w+)").unwrap();
    for cap in tag_re.captures_iter(text) {
        let word = cap.get(1).unwrap();
        facets.push(json!({
            "index": { "byteStart": word.start() - 1, "byteEnd": word.end() },
            "features": [{ "$type": "app.bsky.richtext.facet#tag", "tag": word.as_str() }]
        }));
    }

    Value::Array(facets)
}

fn guess_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

#[async_trait]
impl Provider for Bluesky {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Bluesky
    }
    fn account_id(&self) -> &str {
        &self.account_id
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_text_graphemes: MAX_GRAPHEMES,
            max_media: MAX_IMAGES,
            supports_threads: true,
            supports_alt_text: true,
        }
    }

    async fn verify(&self) -> anyhow::Result<AccountInfo> {
        let s = self.ensure_session().await?;
        Ok(AccountInfo {
            id: self.account_id.clone(),
            provider: ProviderKind::Bluesky,
            handle: s.handle,
            display_name: None,
        })
    }

    fn compose(&self, post: &SourcePost) -> anyhow::Result<PreparedPost> {
        let mut warnings = Vec::new();

        let mut text = post.text.clone();
        if !post.hashtags.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            for (i, tag) in post.hashtags.iter().enumerate() {
                if i > 0 {
                    text.push(' ');
                }
                text.push('#');
                text.push_str(tag);
            }
        }

        let graphemes = text.graphemes(true).count();
        if graphemes > MAX_GRAPHEMES {
            anyhow::bail!("Bluesky: texto de {graphemes} grafemas, máximo {MAX_GRAPHEMES}");
        }

        if post.media.len() > MAX_IMAGES {
            anyhow::bail!(
                "Bluesky: max {MAX_IMAGES} imágenes, recibidas {}",
                post.media.len()
            );
        }

        let mut steps = Vec::new();
        let mut media_refs = Vec::new();
        for (i, m) in post.media.iter().enumerate() {
            let ref_id = format!("img{i}");
            if m.alt.is_none() {
                warnings.push(format!("Imagen {i} sin alt text (accesibilidad)"));
            }
            steps.push(Step::UploadMedia {
                path: m.path.clone(),
                alt: m.alt.clone(),
                ref_id: ref_id.clone(),
            });
            media_refs.push(ref_id);
        }

        let facets = detect_facets(&text);
        steps.push(Step::CreatePost { text, facets, media_refs });

        Ok(PreparedPost {
            account_id: self.account_id.clone(),
            provider: ProviderKind::Bluesky,
            steps,
            warnings,
        })
    }

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        let s = self.ensure_session().await?;

        let mut blobs: std::collections::HashMap<String, (Value, Option<String>)> =
            Default::default();
        let mut post_text = String::new();
        let mut post_facets = Value::Array(vec![]);
        let mut post_media_refs: Vec<String> = Vec::new();

        for step in &prepared.steps {
            match step {
                Step::UploadMedia { path, alt, ref_id } => {
                    let bytes = tokio::fs::read(path).await?;
                    let blob = self.upload_blob(bytes, guess_mime(path)).await?;
                    blobs.insert(ref_id.clone(), (blob, alt.clone()));
                }
                Step::CreatePost { text, facets, media_refs } => {
                    post_text = text.clone();
                    post_facets = facets.clone();
                    post_media_refs = media_refs.clone();
                }
            }
        }

        let mut record = json!({
            "$type": "app.bsky.feed.post",
            "text": post_text,
            "createdAt": Utc::now().to_rfc3339(),
        });

        if post_facets.as_array().map_or(false, |a| !a.is_empty()) {
            record["facets"] = post_facets;
        }

        if !post_media_refs.is_empty() {
            let images: Vec<Value> = post_media_refs
                .iter()
                .filter_map(|r| blobs.get(r))
                .map(|(blob, alt)| json!({ "alt": alt.clone().unwrap_or_default(), "image": blob }))
                .collect();
            record["embed"] = json!({
                "$type": "app.bsky.embed.images",
                "images": images,
            });
        }

        let res: Value = self
            .http
            .post(format!("{PDS}/xrpc/com.atproto.repo.createRecord"))
            .bearer_auth(&s.access_jwt)
            .json(&json!({
                "repo": s.did,
                "collection": "app.bsky.feed.post",
                "record": record,
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let uri = res["uri"].as_str().unwrap_or_default().to_string();
        let rkey = uri.rsplit('/').next().unwrap_or_default();
        let post_url = format!("https://bsky.app/profile/{}/post/{}", s.handle, rkey);

        Ok(PublishResult {
            post_url: Some(post_url),
            platform_id: uri,
            raw: res,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postkit_core::{MediaRef, SourcePost};
    use std::path::PathBuf;

    fn provider() -> Bluesky {
        Bluesky::new("test".into(), "test.bsky.social".into(), "pw".into())
    }

    fn src(text: &str) -> SourcePost {
        SourcePost { text: text.into(), media: vec![], hashtags: vec![] }
    }

    #[test]
    fn compose_basic_post() {
        let result = provider().compose(&src("Hello world")).unwrap();
        assert!(result.warnings.is_empty());
        match &result.steps[0] {
            Step::CreatePost { text, media_refs, .. } => {
                assert_eq!(text, "Hello world");
                assert!(media_refs.is_empty());
            }
            _ => panic!("expected CreatePost"),
        }
    }

    #[test]
    fn compose_appends_hashtags() {
        let source = SourcePost {
            text: "Hello".into(),
            hashtags: vec!["rust".into(), "dev".into()],
            media: vec![],
        };
        let result = provider().compose(&source).unwrap();
        match &result.steps[0] {
            Step::CreatePost { text, .. } => assert_eq!(text, "Hello\n\n#rust #dev"),
            _ => panic!(),
        }
    }

    #[test]
    fn compose_rejects_over_300_graphemes() {
        // 301 grafemas simples
        assert!(provider().compose(&src(&"a".repeat(301))).is_err());
    }

    #[test]
    fn compose_allows_exactly_300_graphemes() {
        assert!(provider().compose(&src(&"a".repeat(300))).is_ok());
    }

    #[test]
    fn compose_counts_emoji_as_one_grapheme() {
        // 1 emoji = 1 grafema; 299 + emoji = 300 → ok
        let text = format!("{}{}", "a".repeat(299), "🦀");
        assert!(provider().compose(&src(&text)).is_ok());
    }

    #[test]
    fn compose_rejects_more_than_4_images() {
        let media = (0..5)
            .map(|i| MediaRef { path: PathBuf::from(format!("img{i}.png")), alt: None })
            .collect();
        let source = SourcePost { text: "test".into(), media, hashtags: vec![] };
        assert!(provider().compose(&source).is_err());
    }

    #[test]
    fn compose_warns_on_missing_alt() {
        let source = SourcePost {
            text: "test".into(),
            media: vec![MediaRef { path: PathBuf::from("img.png"), alt: None }],
            hashtags: vec![],
        };
        let result = provider().compose(&source).unwrap();
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn compose_no_warning_with_alt() {
        let source = SourcePost {
            text: "test".into(),
            media: vec![MediaRef { path: PathBuf::from("img.png"), alt: Some("desc".into()) }],
            hashtags: vec![],
        };
        let result = provider().compose(&source).unwrap();
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn compose_detects_url_facet() {
        let result = provider().compose(&src("Visit https://rust-lang.org please")).unwrap();
        match &result.steps[0] {
            Step::CreatePost { facets, .. } => {
                let arr = facets.as_array().unwrap();
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0]["features"][0]["$type"], "app.bsky.richtext.facet#link");
                assert_eq!(arr[0]["features"][0]["uri"], "https://rust-lang.org");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compose_detects_hashtag_facet() {
        let result = provider().compose(&src("Hello #rust world")).unwrap();
        match &result.steps[0] {
            Step::CreatePost { facets, .. } => {
                let arr = facets.as_array().unwrap();
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0]["features"][0]["$type"], "app.bsky.richtext.facet#tag");
                assert_eq!(arr[0]["features"][0]["tag"], "rust");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compose_facet_byte_offsets_are_correct() {
        // "Visit " = 6 bytes, "https://rust-lang.org" = 21 bytes → end = 27
        let result = provider().compose(&src("Visit https://rust-lang.org end")).unwrap();
        match &result.steps[0] {
            Step::CreatePost { facets, .. } => {
                let f = &facets.as_array().unwrap()[0];
                assert_eq!(f["index"]["byteStart"], 6);
                assert_eq!(f["index"]["byteEnd"], 27);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compose_generates_upload_steps_for_media() {
        let source = SourcePost {
            text: "test".into(),
            media: vec![
                MediaRef { path: PathBuf::from("a.png"), alt: Some("A".into()) },
                MediaRef { path: PathBuf::from("b.png"), alt: Some("B".into()) },
            ],
            hashtags: vec![],
        };
        let result = provider().compose(&source).unwrap();
        assert_eq!(result.steps.len(), 3); // 2 uploads + 1 create
        assert!(matches!(result.steps[0], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[1], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[2], Step::CreatePost { .. }));
    }
}
