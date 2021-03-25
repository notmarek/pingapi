# ------------------------------------------------------------------------------
# Cargo Build Stage
# ------------------------------------------------------------------------------

FROM rust:1.50 as cargo-build

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
ENV TIMEOUT=10
ENV CONNECTIONS=10
ENV CORS="https://piracy.moe"

EXPOSE 5000
HEALTHCHECK CMD curl --fail http://localhost:5000/health || exit 1

CMD redis-server --daemonize yes && pingapi
