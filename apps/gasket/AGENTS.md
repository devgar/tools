
# Devtonainers & Justfile 
This document outlines the standard development workflow for Gasket. To ensure environment parity across different machines and simplify local execution, we use Development Containers (Devcontainers) in conjunction with a Justfile (using the just command runner).

## 1 The devcontainer strategy

The devcontainer provides a consistent, isolated environment with all necessary dependencies (Go 1.21+, Docker SDK, etc.) pre-installed. This avoids "it works on my machine" issues and keeps the host OS clean.
### Configuration
- **Base Image**: Go-based official devcontainer image.
- **Docker-in-Docker**: The host Docker socket is mounted into the container so Gasket can interact with the Docker Engine.
- **Extensions**: Recommended VS Code extensions (Go, Docker, YAML) are automatically installed.

## 2 Justfile Integration

| Command | Description |
| ------- | ----------- |
| build   | Compiles the Gasket binary inside the container. |
| test    | Runs the Go test suite. |
| run     | Starts Gasket with the local development config. |
| clean   | Removes build artifacts and temporary sockets. |

## 3 Running Commands from the Host

The invocations to the devcontainer should be in the justfile scripts so the developer doesn't has to enter to a devcontainer shell to run the just commands.

## 4 Benefits

- **Consistency**: Every developer uses the exact same Go version and tools.
- **Portability**: Works across Linux, macOS, and Windows (via WSL2).
- **Efficiency**: just abstracts away long Docker or Go flags into short, memorable commands.
