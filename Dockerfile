# ------------------------------------------------------------------------------
# Cargo Build Stage
# ------------------------------------------------------------------------------
FROM rust:1.54 as cargo-build

WORKDIR /usr/src/pingapi

# pre-compile deps to take advantage of build caches
COPY Cargo.toml .
RUN mkdir src/ && \
    echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
    cargo build --release && \
    rm -f target/release/deps/pingapi*

COPY . .

RUN cargo build --release && \
    cargo install --path .

# ------------------------------------------------------------------------------
# Final Stage
# ------------------------------------------------------------------------------
FROM debian:buster-slim

COPY --from=cargo-build /usr/local/cargo/bin/pingapi /usr/local/bin/pingapi

# install redis and runtime deps
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        netbase \
        tzdata \
        redis \
        wget && \
    rm -rf /var/lib/apt/lists/*

ENV INTERVAL=300
ENV TIMEOUT=10000
ENV CORS="https://piracy.moe"
# Uncomment to enable proxy support (doesn't have to be socks5)
# ENV SOCKS_IP="socks5://127.0.0.1"
# ENV SOCKS_USER="USER"
# ENV SOCKS_PASS="SECRET"
ENV FLARESOLVERR="FLARE_URL"
ENV SECRET="im vewy secwet"


EXPOSE 8080
HEALTHCHECK CMD curl --fail http://localhost:8080/health || exit 1

LABEL org.opencontainers.image.vendor="/r/animepiracy" \
      org.opencontainers.image.url="https://ping.piracy.moe" \
      org.opencontainers.image.description="Ping API of piracy.moe Index" \
      org.opencontainers.image.title="Ping API" \
      maintainer="Community of /r/animepiracy"

CMD redis-server --daemonize yes && pingapi
