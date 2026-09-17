# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder
WORKDIR /app

# Cache dependencies separately from source changes.
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY . .
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --shell /usr/sbin/nologin appuser

COPY --from=builder /app/target/release/bikepackid_backend /usr/local/bin/bikepackid_backend
COPY migrations ./migrations

USER appuser
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/bikepackid_backend"]
