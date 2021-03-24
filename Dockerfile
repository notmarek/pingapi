# ------------------------------------------------------------------------------
# Cargo Build Stage
# ------------------------------------------------------------------------------

FROM rust:1.50 as cargo-build

WORKDIR /usr/src/pingapi

COPY Cargo.toml Cargo.toml

RUN mkdir src/ && \
    echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
    cargo build --release && \
    rm -f target/release/deps/pingapi*

COPY src/* ./src

RUN cargo build --release && \
    cargo install --path .

# ------------------------------------------------------------------------------
# Final Stage
# ------------------------------------------------------------------------------
FROM python:3.9-slim-buster

COPY --from=cargo-build /usr/local/cargo/bin/pingapi /usr/local/bin/pingapi

# install redis
RUN apt-get update && \
    apt-get install -y --no-install-recommends redis && \
    rm -rf /var/lib/apt/lists/*

# install needed python packages
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

WORKDIR /app
COPY background.py start.sh ./

ENV INTERVAL=300
ENV TIMEOUT=10
ENV CONNECTIONS=10
ENV CORS="https://piracy.moe"

EXPOSE 5000
HEALTHCHECK CMD curl --fail http://localhost:5000/health || exit 1

# sed is for replacing windows newline
CMD sed -i 's/\r$//' start.sh && sh start.sh
