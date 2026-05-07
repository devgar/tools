//! Bluesky provider, usando XRPC directo sobre reqwest.
//!
//! Notas sobre AT Protocol:
//! - Auth: `createSession` con handle + app password, devuelve JWTs.
//!   El accessJwt expira en ~1 hora; se refresca con `refreshSession`.
//! - Texto: max 300 grafemas (no chars, no bytes). Si excede, se crea un hilo.
//! - Facets: para que links/tags/menciones sean clicables hay que anotar byte offsets.
//!   Las menciones se detectan en compose() como `_pending_mention` y se resuelven
//!   a DID en execute() antes de publicar.
//! - Media: subir con `uploadBlob`, luego embeder el objeto blob en el record.

use async_trait::async_trait;
use chrono::Utc;
use postkit_core::*;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use unicode_segmentation::UnicodeSegmentation;

const PDS: &str = "https://bsky.social";
const MAX_GRAPHEMES: usize = 300;
const MAX_IMAGES: usize = 4;
// accessJwt expira en ~1 hora; refrescamos a los 45 min para tener margen.
const ACCESS_JWT_TTL: std::time::Duration = std::time::Duration::from_secs(45 * 60);

/// Respuesta cruda de createSession / refreshSession.
#[derive(Debug, Deserialize)]
struct SessionResponse {
    #[serde(rename = "accessJwt")]
    access_jwt: String,
    #[serde(rename = "refreshJwt")]
    refresh_jwt: String,
    did: String,
    handle: String,
}

/// Sesión activa con marca de tiempo para detectar expiración.
#[derive(Debug, Clone)]
struct Session {
    access_jwt: String,
    refresh_jwt: String,
    did: String,
    handle: String,
    created_at: std::time::Instant,
}

impl From<SessionResponse> for Session {
    fn from(r: SessionResponse) -> Self {
        Session {
            access_jwt: r.access_jwt,
            refresh_jwt: r.refresh_jwt,
            did: r.did,
            handle: r.handle,
            created_at: std::time::Instant::now(),
        }
    }
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

    // ─── XRPC genéricos ──────────────────────────────────────────────────────

    /// POST XRPC. `auth` = None para endpoints sin token (p.ej. createSession).
    async fn xrpc_post<T: DeserializeOwned>(
        &self,
        nsid: &str,
        auth: Option<&str>,
        body: Option<&Value>,
    ) -> anyhow::Result<T> {
        let mut req = self.http.post(format!("{PDS}/xrpc/{nsid}"));
        if let Some(token) = auth {
            req = req.bearer_auth(token);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        Ok(req.send().await?.error_for_status()?.json().await?)
    }

    /// GET XRPC con bearer auth y query params opcionales.
    async fn xrpc_get<T: DeserializeOwned>(
        &self,
        nsid: &str,
        auth: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        Ok(self
            .http
            .get(format!("{PDS}/xrpc/{nsid}"))
            .bearer_auth(auth)
            .query(params)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    // ─── Sesión ───────────────────────────────────────────────────────────────

    async fn ensure_session(&self) -> anyhow::Result<Session> {
        // Fast path: sesión válida en caché.
        {
            let guard = self.session.read().await;
            if let Some(s) = guard.as_ref() {
                if s.created_at.elapsed() < ACCESS_JWT_TTL {
                    return Ok(s.clone());
                }
            }
        }

        // Slow path: crear o refrescar bajo write lock.
        let mut guard = self.session.write().await;

        // Re-check tras adquirir el write lock (otro task puede haber refrescado ya).
        if let Some(s) = guard.as_ref() {
            if s.created_at.elapsed() < ACCESS_JWT_TTL {
                return Ok(s.clone());
            }
            let res: SessionResponse = self
                .xrpc_post("com.atproto.server.refreshSession", Some(&s.refresh_jwt.clone()), None)
                .await?;
            let session = Session::from(res);
            *guard = Some(session.clone());
            return Ok(session);
        }

        let res: SessionResponse = self
            .xrpc_post(
                "com.atproto.server.createSession",
                None,
                Some(&json!({ "identifier": self.handle, "password": self.app_password })),
            )
            .await?;
        let session = Session::from(res);
        *guard = Some(session.clone());
        Ok(session)
    }

    // ─── Operaciones XRPC ────────────────────────────────────────────────────

    async fn upload_blob(&self, bytes: Vec<u8>, mime: &str) -> anyhow::Result<Value> {
        #[derive(Deserialize)]
        struct Res { blob: Value }
        let s = self.ensure_session().await?;
        let res: Res = self
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
        Ok(res.blob)
    }

    async fn resolve_handle(&self, handle: &str) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct Res { did: String }
        let s = self.ensure_session().await?;
        let res: Res = self
            .xrpc_get("com.atproto.identity.resolveHandle", &s.access_jwt, &[("handle", handle)])
            .await?;
        Ok(res.did)
    }

    /// Reemplaza en-sitio los facets `_pending_mention` con menciones AT resueltas.
    /// Los handles que no se puedan resolver se descartan silenciosamente.
    async fn resolve_mentions(&self, facets: &mut Value) {
        let Some(arr) = facets.as_array_mut() else { return };
        for facet in arr.iter_mut() {
            let Some(features) = facet["features"].as_array_mut() else { continue };
            for feature in features.iter_mut() {
                if feature["$type"] != "_pending_mention" { continue; }
                let Some(handle) = feature["handle"].as_str() else { continue };
                let handle = handle.to_string();
                let Ok(did) = self.resolve_handle(&handle).await else { continue };
                *feature = json!({ "$type": "app.bsky.richtext.facet#mention", "did": did });
            }
        }
    }

    /// Publica un record, resolviendo menciones pendientes y encadenando el hilo si procede.
    async fn post_record(
        &self,
        text: &str,
        mut facets: Value,
        media_refs: &[String],
        blobs: &std::collections::HashMap<String, (Value, Option<String>)>,
        reply: Option<Value>,
    ) -> anyhow::Result<PublishResult> {
        self.resolve_mentions(&mut facets).await;

        let mut record = json!({
            "$type": "app.bsky.feed.post",
            "text": text,
            "createdAt": Utc::now().to_rfc3339(),
        });

        if facets.as_array().map_or(false, |a| !a.is_empty()) {
            record["facets"] = facets;
        }

        if !media_refs.is_empty() {
            let images: Vec<Value> = media_refs
                .iter()
                .filter_map(|r| blobs.get(r))
                .map(|(blob, alt)| json!({ "alt": alt.clone().unwrap_or_default(), "image": blob }))
                .collect();
            record["embed"] = json!({ "$type": "app.bsky.embed.images", "images": images });
        }

        if let Some(reply_ref) = reply {
            record["reply"] = reply_ref;
        }

        let s = self.ensure_session().await?;
        let res: Value = self
            .xrpc_post(
                "com.atproto.repo.createRecord",
                Some(&s.access_jwt),
                Some(&json!({
                    "repo": s.did,
                    "collection": "app.bsky.feed.post",
                    "record": record,
                })),
            )
            .await?;

        let uri = res["uri"].as_str().unwrap_or_default().to_string();
        let rkey = uri.rsplit('/').next().unwrap_or_default();
        Ok(PublishResult {
            post_url: Some(format!("https://bsky.app/profile/{}/post/{}", s.handle, rkey)),
            platform_id: uri,
            raw: res,
        })
    }
}

// ─── Funciones puras ─────────────────────────────────────────────────────────

/// Detecta URLs, hashtags y menciones; devuelve el array de facets AT con byte offsets.
/// Las menciones se emiten como `_pending_mention` para resolución diferida en execute().
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

    // (^|\s)@handle — el '@' está en word.start()-1
    let mention_re = regex::Regex::new(r"(?:^|\s)@([a-zA-Z0-9][a-zA-Z0-9._-]*)").unwrap();
    for cap in mention_re.captures_iter(text) {
        let word = cap.get(1).unwrap();
        facets.push(json!({
            "index": { "byteStart": word.start() - 1, "byteEnd": word.end() },
            "features": [{ "$type": "_pending_mention", "handle": word.as_str() }]
        }));
    }

    Value::Array(facets)
}

/// Divide `text` en chunks de `max` grafemas, respetando límites de palabra.
fn split_into_chunks(text: &str, max: usize) -> Vec<String> {
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    if graphemes.len() <= max {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < graphemes.len() {
        let end = (start + max).min(graphemes.len());
        let break_at = if end < graphemes.len() {
            // Buscar el último espacio en la ventana para cortar en límite de palabra.
            (start..end)
                .rev()
                .find(|&i| i > start && graphemes[i].chars().all(char::is_whitespace))
                .unwrap_or(end)
        } else {
            end
        };
        chunks.push(graphemes[start..break_at].concat().trim_end().to_string());
        start = break_at;
        while start < graphemes.len() && graphemes[start].chars().all(char::is_whitespace) {
            start += 1;
        }
    }
    chunks.retain(|c| !c.is_empty());
    chunks
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

// ─── Provider ────────────────────────────────────────────────────────────────

#[async_trait]
impl Provider for Bluesky {
    fn kind(&self) -> ProviderKind { ProviderKind::Bluesky }
    fn account_id(&self) -> &str { &self.account_id }

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
            if !text.is_empty() { text.push_str("\n\n"); }
            for (i, tag) in post.hashtags.iter().enumerate() {
                if i > 0 { text.push(' '); }
                text.push('#');
                text.push_str(tag);
            }
        }

        if post.media.len() > MAX_IMAGES {
            anyhow::bail!("Bluesky: max {MAX_IMAGES} imágenes, recibidas {}", post.media.len());
        }

        let mut steps = Vec::new();
        let mut media_refs = Vec::new();
        for (i, m) in post.media.iter().enumerate() {
            let ref_id = format!("img{i}");
            if m.alt.is_none() {
                warnings.push(format!("Imagen {i} sin alt text (accesibilidad)"));
            }
            steps.push(Step::UploadMedia { path: m.path.clone(), alt: m.alt.clone(), ref_id: ref_id.clone() });
            media_refs.push(ref_id);
        }

        // Dividir en hilo si el texto excede el límite.
        let chunks = split_into_chunks(&text, MAX_GRAPHEMES);
        steps.push(Step::CreatePost {
            text: chunks[0].clone(),
            facets: detect_facets(&chunks[0]),
            media_refs,
        });
        for chunk in chunks.iter().skip(1) {
            steps.push(Step::ThreadContinue {
                text: chunk.clone(),
                facets: detect_facets(chunk),
                media_refs: vec![],
            });
        }

        Ok(PreparedPost { account_id: self.account_id.clone(), provider: ProviderKind::Bluesky, steps, warnings })
    }

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        // Paso 1: subir todos los blobs de media.
        let mut blobs: std::collections::HashMap<String, (Value, Option<String>)> = Default::default();
        for step in &prepared.steps {
            if let Step::UploadMedia { path, alt, ref_id } = step {
                let bytes = tokio::fs::read(path).await?;
                let blob = self.upload_blob(bytes, guess_mime(path)).await?;
                blobs.insert(ref_id.clone(), (blob, alt.clone()));
            }
        }

        // Paso 2: publicar posts en orden, encadenando el hilo.
        let mut thread_root: Option<(String, String)> = None; // (uri, cid)
        let mut thread_parent: Option<(String, String)> = None;
        let mut first_result: Option<PublishResult> = None;

        for step in &prepared.steps {
            match step {
                Step::UploadMedia { .. } => {}
                Step::CreatePost { text, facets, media_refs } => {
                    let res = self.post_record(text, facets.clone(), media_refs, &blobs, None).await?;
                    let uri = res.platform_id.clone();
                    let cid = res.raw["cid"].as_str().unwrap_or_default().to_string();
                    thread_root = Some((uri.clone(), cid.clone()));
                    thread_parent = Some((uri, cid));
                    first_result = Some(res);
                }
                Step::ThreadContinue { text, facets, media_refs } => {
                    let reply = match (&thread_root, &thread_parent) {
                        (Some((root_uri, root_cid)), Some((parent_uri, parent_cid))) => Some(json!({
                            "root":   { "uri": root_uri,   "cid": root_cid },
                            "parent": { "uri": parent_uri, "cid": parent_cid },
                        })),
                        _ => None,
                    };
                    let res = self.post_record(text, facets.clone(), media_refs, &blobs, reply).await?;
                    let uri = res.platform_id.clone();
                    let cid = res.raw["cid"].as_str().unwrap_or_default().to_string();
                    thread_parent = Some((uri, cid));
                }
            }
        }

        first_result.ok_or_else(|| anyhow::anyhow!("Bluesky execute: no hay paso CreatePost"))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use postkit_core::{MediaRef, SourcePost};
    use std::path::PathBuf;

    fn provider() -> Bluesky {
        Bluesky::new("test".into(), "test.bsky.social".into(), "pw".into())
    }

    fn src(text: &str) -> SourcePost {
        SourcePost { text: text.into(), media: vec![], hashtags: vec![], platforms: Default::default() }
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
            platforms: Default::default(),
        };
        let result = provider().compose(&source).unwrap();
        match &result.steps[0] {
            Step::CreatePost { text, .. } => assert_eq!(text, "Hello\n\n#rust #dev"),
            _ => panic!(),
        }
    }

    #[test]
    fn compose_allows_exactly_300_graphemes() {
        assert!(provider().compose(&src(&"a".repeat(300))).is_ok());
    }

    #[test]
    fn compose_counts_emoji_as_one_grapheme() {
        // 299 + emoji = 300 → ok, sin hilo
        let text = format!("{}{}", "a".repeat(299), "🦀");
        let result = provider().compose(&src(&text)).unwrap();
        assert_eq!(result.steps.len(), 1);
        assert!(matches!(result.steps[0], Step::CreatePost { .. }));
    }

    #[test]
    fn compose_splits_long_text_into_thread() {
        // 450 grafemas sin espacios → corte duro: 300 + 150
        let text = "a".repeat(450);
        let result = provider().compose(&src(&text)).unwrap();
        assert_eq!(result.steps.len(), 2);
        assert!(matches!(result.steps[0], Step::CreatePost { .. }));
        assert!(matches!(result.steps[1], Step::ThreadContinue { .. }));
        match &result.steps[0] {
            Step::CreatePost { text, .. } => assert_eq!(text.graphemes(true).count(), 300),
            _ => panic!(),
        }
        match &result.steps[1] {
            Step::ThreadContinue { text, .. } => assert_eq!(text.graphemes(true).count(), 150),
            _ => panic!(),
        }
    }

    #[test]
    fn compose_splits_at_word_boundary() {
        // 280 'a' + espacio + 100 'b' = 381 grafemas → corte en el espacio
        let text = format!("{} {}", "a".repeat(280), "b".repeat(100));
        let result = provider().compose(&src(&text)).unwrap();
        assert_eq!(result.steps.len(), 2);
        match &result.steps[0] {
            Step::CreatePost { text, .. } => assert_eq!(text, &"a".repeat(280)),
            _ => panic!(),
        }
        match &result.steps[1] {
            Step::ThreadContinue { text, .. } => assert_eq!(text, &"b".repeat(100)),
            _ => panic!(),
        }
    }

    #[test]
    fn compose_rejects_more_than_4_images() {
        let media = (0..5)
            .map(|i| MediaRef { path: PathBuf::from(format!("img{i}.png")), alt: None, url: None })
            .collect();
        let source = SourcePost { text: "test".into(), media, hashtags: vec![], platforms: Default::default() };
        assert!(provider().compose(&source).is_err());
    }

    #[test]
    fn compose_warns_on_missing_alt() {
        let source = SourcePost {
            text: "test".into(),
            media: vec![MediaRef { path: PathBuf::from("img.png"), alt: None, url: None }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(!provider().compose(&source).unwrap().warnings.is_empty());
    }

    #[test]
    fn compose_no_warning_with_alt() {
        let source = SourcePost {
            text: "test".into(),
            media: vec![MediaRef { path: PathBuf::from("img.png"), alt: Some("desc".into()), url: None }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(provider().compose(&source).unwrap().warnings.is_empty());
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
    fn compose_detects_mention_facet() {
        let result = provider().compose(&src("Hello @alice.bsky.social!")).unwrap();
        match &result.steps[0] {
            Step::CreatePost { facets, .. } => {
                let arr = facets.as_array().unwrap();
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0]["features"][0]["$type"], "_pending_mention");
                assert_eq!(arr[0]["features"][0]["handle"], "alice.bsky.social");
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
                MediaRef { path: PathBuf::from("a.png"), alt: Some("A".into()), url: None },
                MediaRef { path: PathBuf::from("b.png"), alt: Some("B".into()), url: None },
            ],
            hashtags: vec![],
            platforms: Default::default(),
        };
        let result = provider().compose(&source).unwrap();
        assert_eq!(result.steps.len(), 3); // 2 uploads + 1 create
        assert!(matches!(result.steps[0], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[1], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[2], Step::CreatePost { .. }));
    }
}
