mod client;
mod services;

use async_trait::async_trait;
use client::BskyClient;
use postkit_core::*;
use services::{build_post_text, build_text_steps, prepare_media_steps};

pub struct Bluesky {
    account_id: String,
    client: BskyClient,
}

impl Bluesky {
    pub fn new(account_id: String, handle: String, app_password: String) -> Self {
        Self { account_id, client: BskyClient::new(handle, app_password) }
    }
}

// ─── Provider ────────────────────────────────────────────────────────────────

const MAX_GRAPHEMES: usize = 300;

const MAX_IMAGES: usize = 4;

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
        let s = self.client.ensure_session().await?;
        Ok(AccountInfo {
            id: self.account_id.clone(),
            provider: ProviderKind::Bluesky,
            handle: s.handle,
            display_name: None,
        })
    }

    fn compose(&self, post: &SourcePost) -> anyhow::Result<PreparedPost> {
        let text = build_post_text(&post.text, &post.hashtags);
        let (media_steps, media_refs, warnings) = prepare_media_steps(&post.media, MAX_IMAGES)?;
        let text_steps = build_text_steps(&text, MAX_GRAPHEMES, media_refs);
        let steps = media_steps.into_iter().chain(text_steps).collect();
        Ok(PreparedPost {
            account_id: self.account_id.clone(),
            provider: ProviderKind::Bluesky,
            steps,
            warnings,
        })
    }

    async fn execute(&self, prepared: &PreparedPost) -> anyhow::Result<PublishResult> {
        let blobs = self.upload_media(&prepared.steps).await?;
        self.publish_steps(&prepared.steps, &blobs).await
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use postkit_core::{MediaRef, SourcePost};
    use unicode_segmentation::UnicodeSegmentation;

    use super::*;

    fn provider() -> Bluesky { Bluesky::new("test".into(), "test.bsky.social".into(), "pw".into()) }

    fn src(text: &str) -> SourcePost {
        SourcePost {
            text: text.into(),
            media: vec![],
            hashtags: vec![],
            platforms: Default::default(),
        }
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
        let text = format!("{}{}", "a".repeat(299), "🦀");
        let result = provider().compose(&src(&text)).unwrap();
        assert_eq!(result.steps.len(), 1);
        assert!(matches!(result.steps[0], Step::CreatePost { .. }));
    }

    #[test]
    fn compose_splits_long_text_into_thread() {
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
        let source = SourcePost {
            text: "test".into(),
            media,
            hashtags: vec![],
            platforms: Default::default(),
        };
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
            media: vec![MediaRef {
                path: PathBuf::from("img.png"),
                alt: Some("desc".into()),
                url: None,
            }],
            hashtags: vec![],
            platforms: Default::default(),
        };
        assert!(provider().compose(&source).unwrap().warnings.is_empty());
    }

    #[test]
    fn compose_detects_url_facet() {
        let result = provider()
            .compose(&src("Visit https://rust-lang.org please"))
            .unwrap();
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
        let result = provider()
            .compose(&src("Hello @alice.bsky.social!"))
            .unwrap();
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
        let result = provider()
            .compose(&src("Visit https://rust-lang.org end"))
            .unwrap();
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
        assert_eq!(result.steps.len(), 3);
        assert!(matches!(result.steps[0], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[1], Step::UploadMedia { .. }));
        assert!(matches!(result.steps[2], Step::CreatePost { .. }));
    }
}
