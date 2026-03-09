# Monorepo Context: devgar/tools 🚀

This repository is a monorepo containing personal CLI/TUI tools, automation scripts, and services for the **gar.im** ecosystem.

## 🏗️ Architecture Overview
- **Infrastructure:** Managed via VPS using Docker Compose.
- **Reverse Proxy:** Caddy with `caddy-docker-proxy` plugin.
- **Domain Strategy:** Services are exposed as subdomains of `gar.im`.
- **Command Runner:** `Just` (look for `justfile` in the root).

## 🛠️ Technology Stack
- **Backend/CLI:** Golang or Rust (prefer Rust).
- **Scripting/TUI:** Javascript/Bun.
- **Web/Automation:** JavaScript/Node.js.
- **Tooling:** Git, Docker, VS Code, Antigravity, Helix, Vim.

## 🤖 Agent Instructions & Constraints

### 1. Command Execution & Task Management
- **Rule:** Never suggest manual `docker-compose`, `cargo`, `bun`, `npm`... commands if a `just` recipe exists.
- **Preference:** Always check the `justfile` for existing workflows before proposing new scripts.

### 2. Docker & Caddy Deployment (gar.im)
- All web services must include Caddy labels for automatic discovery.
- **Network:** Use the external `caddy` network.
- **Template for `docker-compose.yml`:**
  ```yaml
  include: [../caddy/cu.network.yaml]
  
  services:
    service-name:
      image: image-name
      labels:
        caddy: service-name.gar.im
        caddy.reverse_proxy: "{{upstreams 80}}"
      networks:
    - caddy