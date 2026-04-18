FROM rust:slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --bin postkit-daemon

# ─────────────────────────────────────────────────────────────────────────────

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/postkit-daemon .

VOLUME ["/data", "/config"]
EXPOSE 8080

ENTRYPOINT ["./postkit-daemon"]
CMD ["--config", "/config/daemon.toml"]
