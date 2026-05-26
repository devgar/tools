# postkit

CLI for composing, publishing, and scheduling social media posts.

## Configuration

Default config path: `~/.config/postkit/config.toml`

See [`config.example.toml`](../../config.example.toml) for the full format.

## Commands

```bash
# List configured accounts and their provider types
postkit accounts

# Verify credentials (API handshake) for one or all accounts
postkit verify
postkit verify mybsky

# Compose a post and print the execution plan as JSON — no publishing
postkit compose post.toml
postkit compose post.toml --targets mybsky my_x

# Publish immediately (bypasses the daemon and scheduler)
postkit publish post.toml
postkit publish post.toml --targets mybsky

# Schedule a post via the daemon
postkit schedule post.toml --at 2026-06-01T10:00:00Z
postkit schedule post.toml --at 2026-06-01T10:00:00Z --targets mybsky
postkit schedule post.toml --at 2026-06-01T10:00:00Z --daemon http://myserver:8080 --api-key secret

# Save a post as a draft via the daemon (no publish date; editable later)
postkit draft post.toml
postkit draft post.toml --targets mybsky
```

## Post file format

```toml
text     = "Hello from postkit 🦀"
hashtags = ["rust", "opensource"]

# Attach images (path for direct publish; url required for Instagram)
# [[media]]
# path = "/path/to/image.png"
# alt  = "Description for accessibility"

# Per-platform text/media overrides (optional)
# [platforms.x]
# text = "Short version for X"

# [platforms.bluesky]
# text     = "Longer Bluesky version"
# hashtags = ["rust", "bluesky"]
```

Platform keys: `bluesky`, `x`, `meta_page`, `meta_instagram`.

## Daemon integration

`schedule` and `draft` forward the post to `postkit-daemon` over HTTP. The daemon URL and API key can be set in config:

```toml
daemon_url     = "http://localhost:8080"
daemon_api_key = "changeme"
```

or passed per-command with `--daemon` and `--api-key`.
