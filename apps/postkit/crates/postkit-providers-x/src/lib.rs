//! X (Twitter) provider, usando X API v2 con OAuth 1.0a (HMAC-SHA1).
//!
//! Notas:
//! - Auth: OAuth 1.0a con app credentials + access token de usuario.
//! - Texto: max 280 caracteres (conteo por grafemas; X en realidad usa NFC+weighting
//!   pero para posts normales el conteo simple es suficiente).
//! - Media: upload a v1.1 (multipart/form-data), tweet a v2 (JSON).
//! - post_url usa /i/web/status/{id} que no requiere conocer el @handle.

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use postkit_core::*;
use rand::{distributions::Alphanumeric, Rng};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sha1::Sha1;
use unicode_segmentation::UnicodeSegmentation;

const MAX_CHARS: usize = 280;
const MAX_IMAGES: usize = 4;
const API_BASE: &str = "https://api.twitter.com";
const UPLOAD_BASE: &str = "https://upload.twitter.com";

pub struct X {
    account_id: String,
    api_key: String,
    api_secret: String,
    access_token: String,
    access_token_secret: String,
    http: Client,
}

impl X {
    pub fn new(
        account_id: String,
        api_key: String,
        api_secret: String,
        access_token: String,
        access_token_secret: String,
    ) -> Self {
        Self {
            account_id,
            api_key,
            api_secret,
            access_token,
            access_token_secret,
            http: Client::new(),
        }
    }

    /// Genera el header Authorization OAuth 1.0a para una petición.
    /// `extra_params`: parámetros de query/form que deben entrar en la firma
    /// (no incluir para multipart/form-data ni para JSON bodies).
    fn oauth_header(&self, method: &str, url: &str, extra_params: &[(&str, &str)]) -> String {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();

        let nonce: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let oauth_fields = [
            ("oauth_consumer_key", self.api_key.as_str()),
            ("oauth_nonce", nonce.as_str()),
            ("oauth_signature_method", "HMAC-SHA1"),
            ("oauth_timestamp", timestamp.as_str()),
            ("oauth_token", self.access_token.as_str()),
            ("oauth_version", "1.0"),
        ];

        // Juntamos oauth + extra params, percent-encoded, ordenados.
        let mut all: Vec<(String, String)> = oauth_fields
            .iter()
            .map(|(k, v)| (pct(k), pct(v)))
            .chain(extra_params.iter().map(|(k, v)| (pct(k), pct(v))))
            .collect();
        all.sort();

        let param_string = all
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");

        let base = format!(
            "{}&{}&{}",
            method.to_uppercase(),
            pct(url),
            pct(&param_string)
        );
        let signing_key = format!("{}&{}", pct(&self.api_secret), pct(&self.access_token_secret));

        let mut mac = Hmac::<Sha1>::new_from_slice(signing_key.as_bytes()).unwrap();
        mac.update(base.as_bytes());
        let sig = pct(&STANDARD.encode(mac.finalize().into_bytes()));

        format!(
            "OAuth oauth_consumer_key=\"{}\", oauth_nonce=\"{}\", oauth_signature=\"{}\", \
             oauth_signature_method=\"HMAC-SHA1\", oauth_timestamp=\"{}\", \
             oauth_token=\"{}\", oauth_version=\"1.0\"",
            pct(&self.api_key),
            pct(&nonce),
            sig,
            timestamp,
            pct(&self.access_token),
        )
    }

    async fn upload_media(&self, bytes: Vec<u8>) -> anyhow::Result<String> {
        let url = format!("{UPLOAD_BASE}/1.1/media/upload.json");
        let auth = self.oauth_header("POST", &url, &[]);
        let form = reqwest::multipart::Form::new()
            .part("media", reqwest::multipart::Part::bytes(bytes));
        let res: Value = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        res["media_id_string"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("X: no media_id_string en respuesta de upload"))
    }
}

// RFC 3986 §2.3: unreserved = ALPHA / DIGIT / "-" / "." / "_" / "~"
// OAuth requires encoding everything else. NON_ALPHANUMERIC encodes "-._~" por
// error — esta constante los deja sin codificar como exige la spec.
const OAUTH_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ').add(b'!').add(b'"').add(b'#').add(b'$').add(b'%')
    .add(b'&').add(b'\'').add(b'(').add(b')').add(b'*').add(b'+')
    .add(b',').add(b'/').add(b':').add(b';').add(b'<').add(b'=')
    .add(b'>').add(b'?').add(b'@').add(b'[').add(b'\\').add(b']')
    .add(b'^').add(b'`').add(b'{').add(b'|').add(b'}');

fn pct(s: &str) -> String {
    utf8_percent_encode(s, OAUTH_ENCODE).to_string()
}


#[async_trait]
impl Provider for X {
    fn kind(&self) -> ProviderKind {
        ProviderKind::X
    }
    fn account_id(&self) -> &str {
        &self.account_id
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_text_graphemes: MAX_CHARS,
            max_media: MAX_IMAGES,
            supports_threads: false,
            supports_alt_text: true,
        }
    }

    async fn verify(&self) -> anyhow::Result<AccountInfo> {
        // v1.1 verify_credentials funciona en Free tier (write-only apps);
        // GET /2/users/me requiere al menos Read access en v2.
        let url = format!("{API_BASE}/1.1/account/verify_credentials.json");
        let auth = self.oauth_header("GET", &url, &[]);

        #[derive(Deserialize)]
        struct Creds {
            screen_name: String,
            name: String,
        }

        let res: Creds = self
            .http
            .get(&url)
            .header("Authorization", auth)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(AccountInfo {
            id: self.account_id.clone(),
            provider: ProviderKind::X,
            handle: res.screen_name,
            display_name: Some(res.name),
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
        if graphemes > MAX_CHARS {
            anyhow::bail!("X: texto de {graphemes} chars, máximo {MAX_CHARS}");
        }

        if post.media.len() > MAX_IMAGES {
            anyhow::bail!("X: max {MAX_IMAGES} imágenes, recibidas {}", post.media.len());
        }

        let mut steps = Vec::new();
        let mut media_refs = Vec::new();
        for (i, m) in post.media.iter().enumerate() {
            let ref_id = format!("img{i}");
            if m.alt.is_none() {
                warnings.push(format!("Imagen {i} sin alt text"));
            }
            steps.push(Step::UploadMedia {
                path: m.path.clone(),
                alt: m.alt.clone(),
                ref_id: ref_id.clone(),
            });
            media_refs.push(ref_id);
        }

        steps.push(Step::CreatePost {
            text,
            facets: Value::Array(vec![]),
            media_refs,
        });

        Ok(PreparedPost {
            account_id: self.account_id.clone(),
            provider: ProviderKind::X,
            steps,
            warnings,
        })
    }

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        let mut media_ids: std::collections::HashMap<String, String> = Default::default();
        let mut post_text = String::new();
        let mut post_media_refs = Vec::new();

        for step in &prepared.steps {
            match step {
                Step::UploadMedia { path, ref_id, .. } => {
                    let bytes = tokio::fs::read(path).await?;
                    let media_id = self.upload_media(bytes).await?;
                    media_ids.insert(ref_id.clone(), media_id);
                }
                Step::CreatePost { text, media_refs, .. } => {
                    post_text = text.clone();
                    post_media_refs = media_refs.clone();
                }
            }
        }

        let url = format!("{API_BASE}/2/tweets");
        let auth = self.oauth_header("POST", &url, &[]);

        let mut body = json!({ "text": post_text });
        if !post_media_refs.is_empty() {
            let ids: Vec<&str> = post_media_refs
                .iter()
                .filter_map(|r| media_ids.get(r).map(String::as_str))
                .collect();
            body["media"] = json!({ "media_ids": ids });
        }

        let res: Value = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let tweet_id = res["data"]["id"].as_str().unwrap_or_default().to_string();
        let post_url = format!("https://x.com/i/web/status/{tweet_id}");

        Ok(PublishResult {
            post_url: Some(post_url),
            platform_id: tweet_id,
            raw: res,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use postkit_core::{MediaRef, SourcePost};
    use std::path::PathBuf;

    fn provider() -> X {
        X::new("test".into(), "key".into(), "secret".into(), "token".into(), "tsecret".into())
    }

    fn src(text: &str) -> SourcePost {
        SourcePost { text: text.into(), media: vec![], hashtags: vec![] }
    }

    #[test]
    fn compose_basic_post() {
        let result = provider().compose(&src("Hello X")).unwrap();
        assert!(result.warnings.is_empty());
        match &result.steps[0] {
            Step::CreatePost { text, media_refs, .. } => {
                assert_eq!(text, "Hello X");
                assert!(media_refs.is_empty());
            }
            _ => panic!("expected CreatePost"),
        }
    }

    #[test]
    fn compose_appends_hashtags() {
        let source = SourcePost {
            text: "Hello".into(),
            hashtags: vec!["rust".into(), "opensource".into()],
            media: vec![],
        };
        let result = provider().compose(&source).unwrap();
        match &result.steps[0] {
            Step::CreatePost { text, .. } => assert_eq!(text, "Hello\n\n#rust #opensource"),
            _ => panic!(),
        }
    }

    #[test]
    fn compose_rejects_over_280_graphemes() {
        assert!(provider().compose(&src(&"a".repeat(281))).is_err());
    }

    #[test]
    fn compose_allows_exactly_280_graphemes() {
        assert!(provider().compose(&src(&"a".repeat(280))).is_ok());
    }

    #[test]
    fn compose_counts_emoji_as_one_grapheme() {
        let text = format!("{}{}", "a".repeat(279), "🦀");
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
    fn compose_no_facets_in_steps() {
        // X no usa facets AT — el array debe estar vacío
        let result = provider().compose(&src("Visit https://example.com")).unwrap();
        match &result.steps[0] {
            Step::CreatePost { facets, .. } => {
                assert!(facets.as_array().unwrap().is_empty());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn compose_generates_upload_steps_for_media() {
        let source = SourcePost {
            text: "test".into(),
            media: vec![MediaRef { path: PathBuf::from("a.jpg"), alt: Some("desc".into()) }],
            hashtags: vec![],
        };
        let result = provider().compose(&source).unwrap();
        assert_eq!(result.steps.len(), 2);
        assert!(matches!(result.steps[0], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[1], Step::CreatePost { .. }));
    }
}
