# postkit

Multi-platform social media scheduler written in Rust.

**`postkit`** — CLI for composing, publishing, and scheduling posts.  
**`postkit-daemon`** — HTTP daemon that holds credentials, runs the job queue, and publishes posts at their scheduled time.

Supported platforms: **Bluesky**, **X (Twitter)**, **Facebook Page**, **Instagram**.

---

## Crate layout

```
crates/
├── postkit-core/               # Provider trait + shared types (SourcePost, PreparedPost, …)
├── postkit-store/              # SQLite persistence (posts, credentials, migrations)
├── postkit-providers-bluesky/  # Bluesky provider (AT Protocol)
├── postkit-providers-x/        # X / Twitter provider (OAuth 1.0a)
├── postkit-providers-meta/     # Facebook Page & Instagram providers (Meta Graph API)
├── postkit/                    # CLI binary
└── postkit-daemon/             # HTTP daemon binary
```

---

## CLI — `postkit`

### Configuration

```
~/.config/postkit/config.toml   (default)
```

See `config.example.toml` for the full format. At minimum you need one `[accounts.*]` entry.

```toml
[accounts.mybsky]
provider    = "bluesky"
handle      = "you.bsky.social"
app_password = "xxxx-xxxx-xxxx-xxxx"   # from bsky.app/settings/app-passwords
```

### Commands

```bash
# List configured accounts
postkit accounts

# Verify credentials (API handshake)
postkit verify
postkit verify mybsky

# Compose a post and print the execution plan as JSON (no publish)
postkit compose post.toml
postkit compose post.toml --targets mybsky my_x

# Publish immediately (skips scheduling)
postkit publish post.toml

# Schedule via the daemon
postkit schedule post.toml --at 2026-06-01T10:00:00Z
postkit schedule post.toml --at 2026-06-01T10:00:00Z --targets mybsky

# Save as draft (no date, editable later)
postkit draft post.toml
```

### Post file format

```toml
# post.toml
text     = "Hello from postkit 🦀"
hashtags = ["rust", "opensource"]

# Optional media
# [[media]]
# path = "/path/to/image.png"
# alt  = "Alt text for accessibility"

# Optional per-platform overrides
# [platforms.x]
# text = "Shorter version for X (max 280 chars)"

# [platforms.bluesky]
# text     = "Longer version with more context"
# hashtags = ["rust", "bluesky", "opensource"]
```

Platform keys: `bluesky`, `x`, `meta_page`, `meta_instagram`.

---

## Daemon — `postkit-daemon`

The daemon exposes an HTTP API and runs a background worker that picks up posts from the queue and publishes them at their scheduled time. It uses SQLite for persistence and optionally Redis for the job queue (falls back to an in-process queue if Redis is unavailable).

### Configuration

The daemon looks for its config file in XDG order:

1. `$XDG_CONFIG_HOME/postkit/daemon.toml` → `~/.config/postkit/daemon.toml`
2. `$XDG_CONFIG_DIRS/postkit/daemon.toml` → `/etc/xdg/postkit/daemon.toml`
3. `--config <path>` flag

**`daemon.toml`:**

```toml
db_path            = "~/.local/share/postkit/postkit.db"
listen             = "127.0.0.1:8080"
accounts_config    = "~/.config/postkit/accounts.toml"

# Optional
api_key            = "changeme"        # required for all protected endpoints
redis_url          = "redis://127.0.0.1:6379"  # omit to use in-process queue
poll_interval_secs = 30                # max sleep between queue checks
max_attempts       = 3                 # retries before marking a post failed
retry_delay_secs   = 60               # base delay between retries
```

**`accounts.toml`** (referenced by `accounts_config`):

```toml
# ─── App credentials (shared across accounts of the same provider) ─────────

[apps.my_x_app]
provider   = "x"
api_key    = "consumer_key"
api_secret = "consumer_secret"

[apps.my_meta_app]
provider = "meta"
app_id     = "..."      # optional; required for Meta token rotation
app_secret = "..."

# ─── Account credentials ───────────────────────────────────────────────────

[accounts.mybsky]
provider     = "bluesky"
handle       = "you.bsky.social"
app_password = "xxxx-xxxx-xxxx-xxxx"

[accounts.myx]
provider             = "x"
app                  = "my_x_app"
access_token         = "..."
access_token_secret  = "..."

[accounts.myfbpage]
provider          = "facebook_page"
app               = "my_meta_app"
page_id           = "123456789012345"
page_access_token = "EAA..."

[accounts.myig]
provider     = "instagram"
app          = "my_meta_app"
ig_user_id   = "123456789012345"
access_token = "EAA..."
```

### HTTP API

All endpoints except `/health` and `/openapi.json` require `X-Api-Key: <api_key>` when `api_key` is set in `daemon.toml`.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Service health check |
| `GET` | `/openapi.json` | OpenAPI 3.1 spec |
| `POST` | `/schedule` | Schedule a post or save as draft |
| `GET` | `/scheduled` | List posts (filter by status, account, date range) |
| `GET` | `/scheduled/{id}` | Get a post |
| `PUT` | `/scheduled/{id}` | Update content or reschedule |
| `DELETE` | `/scheduled/{id}` | Cancel a pending post |
| `POST` | `/scheduled/{id}/retry` | Retry a failed post |

**Post statuses:** `draft` → `pending` → `running` → `published` / `failed`

**Schedule a post:**
```bash
curl -X POST http://localhost:8080/schedule \
  -H "X-Api-Key: changeme" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "mybsky",
    "scheduled_at": "2026-06-01T10:00:00Z",
    "source_post": {
      "text": "Hello from postkit",
      "hashtags": ["rust"],
      "media": []
    }
  }'
```

Omit `scheduled_at` to create a draft.

### OpenAPI spec

The spec is served live at `GET /openapi.json` and also generated at build time:

```bash
just openapi          # writes openapi.json
# or
cargo run --bin generate-openapi > openapi.json
```

---

## Build

```bash
# Debug
cargo build --bin postkit --bin postkit-daemon

# Release (stripped binaries + tarballs + openapi.json)
just package
```

### OCI images

```bash
docker pull ghcr.io/devgar/postkit:latest
docker pull ghcr.io/devgar/postkit-daemon:latest
```

See `Containerfile` for image details. The daemon image expects config at `/config/daemon.toml` and accounts at `/config/accounts.toml`.

---

## Release

Releases are triggered by pushing to a branch named `release/apps/postkit/<bump>`:

```bash
just release patch   # or minor / major
```

GitHub Actions bumps the version, builds and strips the binaries, generates `openapi.json`, creates a GitHub Release with all artifacts (binaries, tarballs, spec, checksums), and pushes OCI images to `ghcr.io/devgar`.
