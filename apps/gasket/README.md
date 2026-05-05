
# Gasket: Project Blueprint & Roadmap

**Gasket** is a high-performance Docker socket proxy written in Go. It is designed to intercept, filter, and transform communication between the Docker Engine and its clients (such as Caddy, Traefik, or monitoring tools).

## 🎯 Core Objectives
* **Dynamic Translation:** Transform container labels on the fly to ensure compatibility between different reverse proxies and services.
* **Security Isolation:** Act as an API Firewall to prevent exposing the full power of the Docker socket to untrusted services.
* **Multi-Tenancy:** Expose multiple virtual sockets, each with its own granular permission set and transformation rules.

---

## 🛠 Phase 1: MVP (Minimum Viable Product)
*Goal: A functional proxy that translates labels for Caddy.*

- [ ] **Docker SDK Integration:** Set up the official Docker Go SDK to communicate with the upstream Unix Socket.
- [ ] **Proxy Engine:** Build an HTTP server that listens on a Unix socket (e.g., `/tmp/gasket.sock`) and forwards requests to the real engine.
- [ ] **`/containers/json` Interceptor:**
    - Deserialize the Docker Engine's response.
    - Implement label injection logic (e.g., mapping `gasket.target` -> `caddy.address`).
- [ ] **Streaming Pass-through:** Implement `io.Copy` for non-intercepted endpoints (like logs or stats) to maintain zero-latency.
- [ ] **Dockerization:** Create a multi-stage Dockerfile to produce a minimal, scratch-based static binary.

---

## 🔒 Phase 2: Security & Filtering
*Goal: Harden the socket and reduce the attack surface.*

- [ ] **Method/Path Whitelisting:** Block all requests by default, allowing only specific paths (e.g., only `GET` requests to `/containers/json`).
- [ ] **ReadOnly Enforcement:** Ensure the proxy cannot perform write operations (`POST`, `DELETE`, `PATCH`) unless explicitly configured for a specific socket.
- [ ] **Event Masking:** Filter the `/events` stream so clients only receive notifications for containers they are authorized to see.

---

## 🚀 Phase 3: Multi-Socket & RBAC
*Goal: Advanced management for multiple clients.*

- [ ] **Configuration Provider:** Implement YAML/TOML configuration to define multiple virtual sockets.
- [ ] **Virtual Sockets:** Support for spawning $N$ simultaneous Unix Socket listeners from a single Gasket instance.
- [ ] **Socket-Specific Policies:**
    - `socket_caddy.sock`: Read-only, label translation enabled.
    - `socket_monitor.sock`: Access limited to `/stats` and `/version` only.
- [ ] **TUI Dashboard:** A terminal-based interface to monitor intercepted requests and transformation hits in real-time.

---

## 🏗 Technical Architecture

### Stack
* **Language:** Go 1.21+
* **Key Libraries:**
    * `github.com/docker/docker/client`: Official SDK.
    * `net/http`: Standard library for robust proxying.
    * `go.uber.org/zap`: High-performance structured logging.

### Data Flow
1.  **Listener:** Accepts a connection on a virtual socket (e.g., `/tmp/gasket_caddy.sock`).
2.  **Auth/Policy:** Validates the requested path and method against the defined whitelist.
3.  **Upstream:** Gasket requests the data from the real `/var/run/docker.sock`.
4.  **Transformer:** If the response is the container list, the JSON is modified according to the translation rules.
5.  **Downstream:** The modified response is streamed back to the client.

---

## 📂 Repository Structure
```text
.
├── cmd/
│   └── gasket/          # Main entry point
├── internal/
│   ├── proxy/           # Proxy engine logic
│   ├── transformer/     # Label translation and JSON manipulation
│   └── config/          # Policy and socket configuration management
├── deployments/         # Dockerfile and Compose examples
├── agents.md            # Project roadmap
└── main.go
```

