# syntax=docker/dockerfile:1

FROM rust:1.90-bookworm AS builder

WORKDIR /app

# The native dependencies are needed by Songbird's voice implementation while
# compiling its transitive dependencies.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --locked

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /app yomiage

WORKDIR /app
COPY --from=builder /app/target/release/yomiage /usr/local/bin/yomiage

USER yomiage

# Keep synthesized audio outside the image filesystem.  In Compose this is
# backed by the named `voicevox-cache` volume.
ENV VOICEVOX_DISK_CACHE_DIR=/app/.voicevox-cache

# The application handles Ctrl+C by leaving connected voice channels. Docker
# normally sends SIGTERM, so request SIGINT during `docker stop` instead.
STOPSIGNAL SIGINT

ENTRYPOINT ["yomiage"]
