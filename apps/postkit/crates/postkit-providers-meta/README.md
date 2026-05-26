# postkit-providers-meta

Meta Graph API providers for postkit: **Facebook Page** and **Instagram Business/Creator**.

API base: `https://graph.facebook.com/v25.0`

## Facebook Page

```rust
use postkit_providers_meta::FacebookPage;

let provider = FacebookPage::new(
    "account_id".into(),
    "123456789012345".into(), // Page ID
    "EAA...".into(),          // Page Access Token
)
.with_token_sink(store_arc)                          // optional: persist refreshed tokens
.with_app_credentials("app_id".into(), "secret".into()); // optional: enable token rotation
```

| Resource | Limit |
|----------|-------|
| Text | 63,206 graphemes |
| Images | 10 per post |

## Instagram

```rust
use postkit_providers_meta::Instagram;

let provider = Instagram::new(
    "account_id".into(),
    "123456789012345".into(), // Instagram User ID
    "EAA...".into(),          // User Access Token
)
.with_token_sink(store_arc)
.with_app_credentials("app_id".into(), "secret".into());
```

| Resource | Limit |
|----------|-------|
| Caption | 2,200 graphemes |
| Images | 10 per post (carousel) |
| Media URL | **Required** — each image must have a public `url` field; local paths are not supported |

## Token rotation

When `with_app_credentials(app_id, app_secret)` is set, `ensure_fresh_token()` exchanges the current token for a long-lived one via `fb_exchange_token`. The daemon calls this at startup if the stored token is within 7 days of expiry.

- Long-lived tokens are assumed valid for 60 days after exchange.
- Refreshed tokens are persisted via `TokenSink` (usually `postkit-store`).
- If no app credentials are configured, tokens are used as-is (no rotation).
