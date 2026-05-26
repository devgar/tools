# postkit-providers-bluesky

Bluesky provider for postkit using the AT Protocol (XRPC).

## Limits

| Resource | Limit |
|----------|-------|
| Text | 300 graphemes |
| Images | 4 per post |
| Threads | Supported (long posts are automatically split) |

## Usage

```rust
use postkit_providers_bluesky::Bluesky;

let provider = Bluesky::new(
    "account_id".into(),
    "you.bsky.social".into(),
    "xxxx-xxxx-xxxx-xxxx".into(), // app password
);

// Optional: persist session tokens across restarts
let provider = provider.with_token_sink(store_arc);
```

## Auth

Uses Bluesky **app passwords** — never your main account password. Generate one at `bsky.app/settings/app-passwords`.

Session tokens (`accessJwt` / `refreshJwt`) are managed internally by `BskyClient`:

- `accessJwt` expires in ~2 hours. The client refreshes it automatically via `com.atproto.server.refreshSession`.
- `refreshJwt` lasts ~90 days (client uses an 80-day threshold for safety). After expiry, a full re-login with the app password is performed.
- If a `TokenSink` is provided, the refreshed tokens are persisted so the daemon survives restarts without a fresh login.

## Internals

| Module | Responsibility |
|--------|---------------|
| `lib.rs` | `Provider` trait implementation |
| `client.rs` | XRPC session management, blob upload, record creation |
| `services.rs` | Pure compose functions: text building, media steps, thread splitting |
