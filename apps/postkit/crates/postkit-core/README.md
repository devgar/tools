# postkit-core

Shared types and traits used by all postkit crates.

## Three-stage provider model

Every platform provider implements three operations:

| Stage | Method | Description |
|-------|--------|-------------|
| 1 | `verify()` | Validates credentials against the platform API. Returns `AccountInfo`. |
| 2 | `compose()` | **Pure function.** Converts a `SourcePost` into a `PreparedPost` (a list of `Step`s). No I/O, no side effects — safe to call for previewing a plan before publishing. |
| 3 | `execute()` | Runs the `Step`s against the platform API and returns the post URL. |

## Key types

### Input

```rust
pub struct SourcePost {
    pub text: String,
    pub media: Vec<MediaRef>,
    pub hashtags: Vec<String>,
    // Per-platform overrides keyed by ProviderKind::config_key()
    pub platforms: HashMap<String, SourcePostOverride>,
}
```

Call `source.resolve(provider.kind())` to get a `SourcePost` with the platform-specific overrides merged in before passing it to `compose()`.

### Output of `compose()`

```rust
pub struct PreparedPost {
    pub provider: ProviderKind,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

pub enum Step {
    UploadMedia { path: PathBuf, alt: Option<String> },
    CreatePost  { text: String, facets: Option<serde_json::Value>, media_refs: Vec<usize> },
    ThreadContinue { text: String, facets: Option<serde_json::Value>, media_refs: Vec<usize> },
}
```

### Token persistence

Providers that refresh OAuth tokens implement `TokenSink` (provided by `postkit-store`):

```rust
#[async_trait]
pub trait TokenSink: Send + Sync {
    async fn load(&self, account_id: &str) -> anyhow::Result<Option<TokenSet>>;
    async fn save(&self, account_id: &str, tokens: &TokenSet) -> anyhow::Result<()>;
}
```

## Supported platform keys

| `ProviderKind` | `config_key()` |
|----------------|----------------|
| `Bluesky` | `"bluesky"` |
| `X` | `"x"` |
| `MetaPage` | `"meta_page"` |
| `MetaInstagram` | `"meta_instagram"` |
