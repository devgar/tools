# postkit-providers-x

X (Twitter) provider for postkit using X API v2 with OAuth 1.0a (HMAC-SHA1).

## Limits

| Resource | Limit |
|----------|-------|
| Text | 280 characters (grapheme count) |
| Images | 4 per post |
| Threads | Not supported |

## Usage

```rust
use postkit_providers_x::X;

let provider = X::new(
    "account_id".into(),
    "consumer_key".into(),
    "consumer_secret".into(),
    "access_token".into(),
    "access_token_secret".into(),
);
```

## Auth

OAuth 1.0a with app credentials (consumer key/secret) and per-user access tokens. Credentials are obtained from the [X Developer Portal](https://developer.twitter.com).

Tokens do not expire under normal use — no token refresh is needed.

## API usage

| Operation | API |
|-----------|-----|
| Media upload | v1.1 `POST /media/upload` (multipart/form-data) |
| Post creation | v2 `POST /tweets` (JSON) |
| Credential verify | v1.1 `GET /account/verify_credentials` |
| Post URL | `https://x.com/i/web/status/{id}` (no handle required) |
