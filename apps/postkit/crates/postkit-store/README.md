# postkit-store

SQLite persistence layer for scheduled posts and OAuth credentials.

## Features

- WAL journal mode + 5-second busy timeout — safe for concurrent HTTP server + worker access.
- Migrations run automatically on `Store::open()`.
- Optional `openapi` feature: derives `utoipa::ToSchema` on `ScheduledPost`.

## Opening the store

```rust
let store = Store::open("/path/to/postkit.db").await?;
// or in-memory for tests:
let store = Store::open(":memory:").await?;
```

Parent directories are created automatically.

## Post lifecycle

```
draft ──► pending ──► running ──► published
                  └──────────► failed
```

| Method | Description |
|--------|-------------|
| `schedule(account, provider, source_json, at)` | Creates a `pending` post. |
| `create_draft(account, provider, source_json)` | Creates a `draft` post (no date). |
| `update(id, source_json?, scheduled_at?)` | Updates content or date. Promotes a `draft` to `pending` when a date is provided. |
| `cancel(id)` | Marks a `pending` post as `cancelled`. |
| `retry(id)` | Resets a `failed` post to `pending`. |
| `claim_by_ids(ids)` | Atomically transitions `pending → running` for the given IDs. Used by the worker. |
| `mark_published(id, url?)` | Transitions `running → published`. |
| `attempt_or_fail(id, error, max, delay)` | Increments attempt counter; transitions to `failed` after `max` attempts. |

## Querying

```rust
let posts = store.list(&ListFilters {
    account_id: Some("mybsky".into()),
    status: Some("pending".into()),
    from: None,
    to: None,
    provider: None,
    limit: Some(50),
    offset: None,
}).await?;
```

## Credential storage

Used by Meta and Bluesky providers to persist refreshed tokens across restarts:

```rust
store.save_credential("account_id", &TokenSet { access_token, refresh_token, expires_at }).await?;
let tokens = store.load_credential("account_id").await?;
```

`Store` implements `postkit_core::TokenSink` directly.
