# Build context is the repo root (not journey-service/) — this crate
# depends on the workspace-sibling `common` crate via a path dependency,
# which needs to be visible in the build context. See
# .github/workflows/ci.yml for the `context: .` / `file:
# journey-service/Dockerfile` pairing that makes this work.

# ---- Build stage ----
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# openssl-sys (pulled in by sqlx's native-tls runtime) needs these to
# compile.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation in its own layer: this only re-runs when a
# Cargo.toml/Cargo.lock changes, not on every source edit. `auth-service`'s
# manifest AND a dummy target have to be present too — the workspace root
# Cargo.toml lists it as a member, so Cargo needs to see a valid package
# there (a manifest alone isn't enough — it also needs something Cargo can
# infer a target from) to resolve the workspace graph — but its real
# source is never copied in or compiled here, only journey-service and its
# `common` dependency are.
COPY Cargo.toml Cargo.lock ./
COPY auth-service/Cargo.toml auth-service/Cargo.toml
COPY common/Cargo.toml common/Cargo.toml
COPY journey-service/Cargo.toml journey-service/Cargo.toml
RUN mkdir -p auth-service/src common/src journey-service/src \
    && echo "fn main() {}" > auth-service/src/main.rs \
    && echo "" > common/src/lib.rs \
    && echo "fn main() {}" > journey-service/src/main.rs
# Only common/src and journey-service/src get replaced with real source
# below — auth-service/src's dummy stays put for the rest of this build;
# it's never compiled again and never copied into the runtime stage, so
# there's nothing to gain by removing it (and removing it would break the
# second `cargo build` below the same way the first COPY's absence did).
RUN cargo build --release -p bikepackid_journey_service && rm -rf common/src journey-service/src

COPY common/src common/src
COPY journey-service/src journey-service/src

# Set right before the real build (not above, next to the dependency-only
# build) so a changing commit SHA doesn't invalidate the dependency-layer
# cache. Baked in via `option_env!` in src/routes.rs, not read at
# container runtime — exposed at GET /version to confirm what's deployed
# without SSHing in to inspect Docker image digests.
ARG GIT_SHA=unknown
ENV GIT_SHA=$GIT_SHA

# Without this, BuildKit's COPY preserves each file's mtime from git
# rather than setting it to "now" — so the real common/src (restored
# above) can end up looking *older* than the stale rlib cargo already
# built from common's empty dummy lib.rs in the dependency-only build
# above, and cargo skips rebuilding it, silently linking journey-service
# against a bikepackid_common with none of its real modules. Touch every
# real source file just copied in (not just this crate's own entrypoint)
# so cargo always sees them as newer than anything already built.
RUN find journey-service/src common/src -name '*.rs' -exec touch {} + \
    && cargo build --release -p bikepackid_journey_service

# ---- Runtime stage ----
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/bikepackid_journey_service ./bikepackid_journey_service

EXPOSE 8081
CMD ["./bikepackid_journey_service"]
