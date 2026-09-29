# Build context is the repo root (not backend/) — backend depends on the
# workspace-sibling `common` crate via a path dependency, which needs to be
# visible in the build context. See .github/workflows/ci.yml for the
# `context: .` / `file: backend/Dockerfile` pairing that makes this work.

# ---- Build stage ----
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# openssl-sys (pulled in by sqlx's native-tls runtime and reqwest's default
# TLS backend) needs these to compile.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation in its own layer: this only re-runs when a
# Cargo.toml/Cargo.lock changes, not on every source edit — the dummy
# src files exist purely so `cargo build` has something to compile against
# the locked dependency graph for both workspace members.
COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml backend/Cargo.toml
COPY common/Cargo.toml common/Cargo.toml
RUN mkdir -p backend/src common/src \
    && echo "fn main() {}" > backend/src/main.rs \
    && echo "" > common/src/lib.rs
RUN cargo build --release -p bikepackid_backend && rm -rf backend/src common/src

COPY backend/src backend/src
COPY backend/migrations backend/migrations
COPY common/src common/src

# Set right before the real build (not above, next to the dependency-only
# build) so a changing commit SHA doesn't invalidate the dependency-layer
# cache. Baked in via `option_env!` in src/routes/health.rs, not read at
# container runtime — exposed at GET /version to confirm what's deployed
# without SSHing in to inspect Docker image digests.
ARG GIT_SHA=unknown
ENV GIT_SHA=$GIT_SHA

# Without this, cargo may see main.rs's mtime as older than the cached
# build output and skip recompiling the real source.
RUN touch backend/src/main.rs && cargo build --release -p bikepackid_backend

# ---- Runtime stage ----
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/bikepackid_backend ./bikepackid_backend
COPY --from=builder /app/backend/migrations ./migrations

EXPOSE 8080
CMD ["./bikepackid_backend"]
