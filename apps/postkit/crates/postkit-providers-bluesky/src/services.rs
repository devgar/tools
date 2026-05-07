//! Servicios de Bluesky: lógica de compose/execute extraída del Provider.
//!
//! Dos capas:
//! - Funciones puras (`build_post_text`, `prepare_media_steps`, `build_text_steps`)
//!   que no necesitan I/O y se pueden llamar directamente desde el Provider.
//! - Métodos adicionales sobre `Bluesky` (`upload_media`, `publish_steps`, `post_record`)
//!   que necesitan `self.client` y se llaman como `self.método()` desde el Provider.

use super::Bluesky;
use chrono::Utc;
use postkit_core::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) type BlobMap = HashMap<String, (Value, Option<String>)>;

// ─── Funciones puras ──────────────────────────────────────────────────────────

/// Añade hashtags al texto con separador de párrafo.
pub(crate) fn build_post_text(text: &str, hashtags: &[String]) -> String {
    if hashtags.is_empty() {
        return text.to_string();
    }
    let mut result = text.to_string();
    if !result.is_empty() { result.push_str("\n\n"); }
    for (i, tag) in hashtags.iter().enumerate() {
        if i > 0 { result.push(' '); }
        result.push('#');
        result.push_str(tag);
    }
    result
}

/// Valida el límite de imágenes y construye los pasos `UploadMedia`.
/// Devuelve `(upload_steps, media_refs, warnings)`.
pub(crate) fn prepare_media_steps(
    media: &[MediaRef],
    max: usize,
) -> anyhow::Result<(Vec<Step>, Vec<String>, Vec<String>)> {
    if media.len() > max {
        anyhow::bail!("Bluesky: max {max} imágenes, recibidas {}", media.len());
    }
    let mut steps = Vec::new();
    let mut media_refs = Vec::new();
    let mut warnings = Vec::new();
    for (i, m) in media.iter().enumerate() {
        let ref_id = format!("img{i}");
        if m.alt.is_none() {
            warnings.push(format!("Imagen {i} sin alt text (accesibilidad)"));
        }
        steps.push(Step::UploadMedia { path: m.path.clone(), alt: m.alt.clone(), ref_id: ref_id.clone() });
        media_refs.push(ref_id);
    }
    Ok((steps, media_refs, warnings))
}

/// Divide el texto en chunks y construye los pasos `CreatePost` + `ThreadContinue`.
/// `media_refs` se asigna al primer paso (imagen del post raíz).
pub(crate) fn build_text_steps(text: &str, max: usize, media_refs: Vec<String>) -> Vec<Step> {
    let chunks = split_into_chunks(text, max);
    let mut steps = Vec::with_capacity(chunks.len());
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
    steps
}

fn detect_facets(text: &str) -> Value {
    let mut facets: Vec<Value> = Vec::new();

    let url_re = regex::Regex::new(r"https?://[^\s]+").unwrap();
    for m in url_re.find_iter(text) {
        facets.push(json!({
            "index": { "byteStart": m.start(), "byteEnd": m.end() },
            "features": [{ "$type": "app.bsky.richtext.facet#link", "uri": m.as_str() }]
        }));
    }

    let tag_re = regex::Regex::new(r"(?:^|\s)#(\w+)").unwrap();
    for cap in tag_re.captures_iter(text) {
        let word = cap.get(1).unwrap();
        facets.push(json!({
            "index": { "byteStart": word.start() - 1, "byteEnd": word.end() },
            "features": [{ "$type": "app.bsky.richtext.facet#tag", "tag": word.as_str() }]
        }));
    }

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

// ─── Métodos sobre Bluesky ────────────────────────────────────────────────────

impl Bluesky {
    /// Sube todos los blobs de media y devuelve el mapa ref_id → (blob, alt).
    pub(crate) async fn upload_media(&self, steps: &[Step]) -> anyhow::Result<BlobMap> {
        let mut blobs: BlobMap = Default::default();
        for step in steps {
            if let Step::UploadMedia { path, alt, ref_id } = step {
                let bytes = tokio::fs::read(path).await?;
                let blob = self.client.upload_blob(bytes, guess_mime(path)).await?;
                blobs.insert(ref_id.clone(), (blob, alt.clone()));
            }
        }
        Ok(blobs)
    }

    /// Publica los pasos de texto en orden, encadenando el hilo si hay varios.
    /// Devuelve el `PublishResult` del post raíz.
    pub(crate) async fn publish_steps(
        &self,
        steps: &[Step],
        blobs: &BlobMap,
    ) -> anyhow::Result<PublishResult> {
        let mut thread_root: Option<(String, String)> = None; // (uri, cid)
        let mut thread_parent: Option<(String, String)> = None;
        let mut first_result: Option<PublishResult> = None;

        for step in steps {
            match step {
                Step::UploadMedia { .. } => {}
                Step::CreatePost { text, facets, media_refs } => {
                    let res = self.post_record(text, facets.clone(), media_refs, blobs, None).await?;
                    let cid = res.raw["cid"].as_str().unwrap_or_default().to_string();
                    thread_root = Some((res.platform_id.clone(), cid.clone()));
                    thread_parent = Some((res.platform_id.clone(), cid));
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
                    let res = self.post_record(text, facets.clone(), media_refs, blobs, reply).await?;
                    let cid = res.raw["cid"].as_str().unwrap_or_default().to_string();
                    thread_parent = Some((res.platform_id, cid));
                }
            }
        }

        first_result.ok_or_else(|| anyhow::anyhow!("Bluesky execute: no hay paso CreatePost"))
    }

    /// Construye el JSON del record, resuelve menciones y lo publica.
    async fn post_record(
        &self,
        text: &str,
        mut facets: Value,
        media_refs: &[String],
        blobs: &BlobMap,
        reply: Option<Value>,
    ) -> anyhow::Result<PublishResult> {
        self.client.resolve_mentions(&mut facets).await;

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

        let r = self.client.create_record(record).await?;
        let rkey = r.uri.rsplit('/').next().unwrap_or_default();
        Ok(PublishResult {
            post_url: Some(format!("https://bsky.app/profile/{}/post/{}", r.author_handle, rkey)),
            platform_id: r.uri,
            raw: r.raw,
        })
    }
}
