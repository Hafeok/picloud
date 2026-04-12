# ── Stage 1: Build ────────────────────────────────────────────────────────────
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Copy manifests first for better layer caching
COPY Cargo.toml ./
COPY crates/ crates/
COPY src/ src/

RUN cargo build --workspace --release

# ── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/picloud-server /usr/local/bin/
COPY --from=builder /src/target/release/picloud        /usr/local/bin/

RUN mkdir -p /var/lib/picloud

EXPOSE 7443

ENTRYPOINT ["picloud-server"]
