# postkit-daemon

HTTP daemon that holds provider credentials, manages a job queue, and publishes posts at their scheduled time.

## Architecture

```
┌──────────────┐   HTTP    ┌─────────────────────────────────────────┐
│  postkit CLI │ ────────► │             postkit-daemon               │
│  (or curl)   │           │                                          │
└──────────────┘           │  ┌──────────┐   ┌──────────────────┐   │
                           │  │  axum    │   │  worker task     │   │
                           │  │  server  │   │  pop_due() loop  │   │
                           │  └────┬─────┘   └────────┬─────────┘   │
                           │       │                   │             │
                           │  ┌────▼───────────────────▼─────────┐  │
                           │  │         SQLite (postkit-store)    │  │
                           │  └───────────────────────────────────┘  │
                           │  ┌────────────────────────────────────┐  │
                           │  │   Job queue: Redis or MemoryQueue  │  │
                           │  └────────────────────────────────────┘  │
                           └─────────────────────────────────────────┘
```

The HTTP server and worker run concurrently. SQLite WAL mode prevents them from blocking each other.

## Configuration

Config file is resolved in XDG order:

1. `$XDG_CONFIG_HOME/postkit/daemon.toml` → `~/.config/postkit/daemon.toml`
2. `$XDG_CONFIG_DIRS/postkit/daemon.toml` → `/etc/xdg/postkit/daemon.toml`
3. `--config <path>` flag override

**`daemon.toml` reference:**

```toml
# Required
db_path         = "~/.local/share/postkit/postkit.db"
listen          = "127.0.0.1:8080"
accounts_config = "~/.config/postkit/accounts.toml"

# Optional
api_key            = "changeme"            # omit to disable auth (local dev only)
redis_url          = "redis://127.0.0.1:6379"  # omit to use in-process queue
poll_interval_secs = 30                    # max idle time between queue checks
max_attempts       = 3                     # retries before marking a post failed
retry_delay_secs   = 60                    # seconds between retries
```

See the root [`config.example.toml`](../../config.example.toml) for the `accounts_config` format.

## Job queue

The worker drains due jobs from the queue on every loop tick. Queue selection at startup:

- **Redis** — if `redis_url` is set and the connection succeeds. Durable across restarts; supports multiple workers.
- **MemoryQueue** — in-process fallback (default). Repopulated from SQLite on restart.

On startup, all `pending` posts from SQLite are pushed into the queue regardless of which backend is used.

## HTTP API

All endpoints except `/health` and `/openapi.json` require `X-Api-Key: <value>` when `api_key` is configured.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Liveness check → `{"status":"ok","version":"..."}` |
| `GET` | `/openapi.json` | OpenAPI 3.1 spec (unauthenticated) |
| `POST` | `/schedule` | Schedule a post or create a draft |
| `GET` | `/scheduled` | List posts with optional filters |
| `GET` | `/scheduled/{id}` | Get a single post |
| `PUT` | `/scheduled/{id}` | Update content or reschedule |
| `DELETE` | `/scheduled/{id}` | Cancel a pending post |
| `POST` | `/scheduled/{id}/retry` | Retry a failed post |

### Query filters for `GET /scheduled`

`account_id`, `provider`, `status`, `from`, `to`, `limit`, `offset`

### Post status flow

```
draft ──► pending ──► running ──► published
                  └──────────► failed
```

## OpenAPI spec

The spec is available at runtime via `GET /openapi.json` and can also be generated statically:

```bash
cargo run --bin generate-openapi > openapi.json
# or
just openapi
```

## Running

```bash
# Uses ~/.config/postkit/daemon.toml by default
postkit-daemon

# Explicit config path
postkit-daemon --config /etc/postkit/daemon.toml
```

```
RUST_LOG=postkit_daemon=debug postkit-daemon
```
