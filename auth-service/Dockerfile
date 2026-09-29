# Build context is the repo root (not auth-service/) — auth-service
# depends on the workspace-sibling `common` crate via a path dependency,
# which needs to be visible in the build context. See
# .github/workflows/ci.yml for the `context: .` / `file:
# auth-service/Dockerfile` pairing that makes this work.

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
# the locked dependency graph. `journey-service`'s manifest+dummy target
# have to be present too — the workspace root Cargo.toml lists it as a
# member, so Cargo needs to see a valid package there to resolve the
# workspace graph — but its real source is never copied in or compiled
# here, only auth-service and its `common` dependency are.
COPY Cargo.toml Cargo.lock ./
COPY auth-service/Cargo.toml auth-service/Cargo.toml
COPY common/Cargo.toml common/Cargo.toml
COPY journey-service/Cargo.toml journey-service/Cargo.toml
RUN mkdir -p auth-service/src common/src journey-service/src \
    && echo "fn main() {}" > auth-service/src/main.rs \
    && echo "" > common/src/lib.rs \
    && echo "fn main() {}" > journey-service/src/main.rs
# Only auth-service/src and common/src get replaced with real source
# below — journey-service/src's dummy stays put for the rest of this
# build; it's never compiled again and never copied into the runtime
# stage, so there's nothing to gain by removing it (and removing it
# would break the second `cargo build` below the same way the first
# COPY's absence did).
RUN cargo build --release -p bikepackid_auth_service && rm -rf auth-service/src common/src

COPY auth-service/src auth-service/src
COPY auth-service/migrations auth-service/migrations
COPY common/src common/src

# Set right before the real build (not above, next to the dependency-only
# build) so a changing commit SHA doesn't invalidate the dependency-layer
# cache. Baked in via `option_env!` in src/routes/health.rs, not read at
# container runtime — exposed at GET /version to confirm what's deployed
# without SSHing in to inspect Docker image digests.
ARG GIT_SHA=unknown
ENV GIT_SHA=$GIT_SHA

# Without this, BuildKit's COPY preserves each file's mtime from git
# rather than setting it to "now" — so the real common/src (restored
# above) can end up looking *older* than the stale rlib cargo already
# built from common's empty dummy lib.rs in the dependency-only build
# above, and cargo skips rebuilding it, silently linking auth-service
# against a bikepackid_common with none of its real modules. Touch every
# real source file just copied in (not just auth-service's own
# entrypoint) so cargo always sees them as newer than anything already
# built.
RUN find auth-service/src common/src -name '*.rs' -exec touch {} + \
    && cargo build --release -p bikepackid_auth_service

# ---- Runtime stage ----
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/bikepackid_auth_service ./bikepackid_auth_service
COPY --from=builder /app/auth-service/migrations ./migrations

EXPOSE 8080
CMD ["./bikepackid_auth_service"]
