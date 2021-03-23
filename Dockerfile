FROM rust as builder

RUN apt-get update && \
    apt-get install musl-tools -y && \
    rustup target add x86_64-unknown-linux-musl

WORKDIR /usr/src/pingapi

COPY Cargo.toml Cargo.toml

RUN mkdir src/ && \
    echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
    RUSTFLAGS=-Clinker=musl-gcc cargo build --release --target=x86_64-unknown-linux-musl && \
    rm -f target/x86_64-unknown-linux-musl/release/deps/pingapi*

COPY src/* ./src

RUN RUSTFLAGS=-Clinker=musl-gcc cargo build --release --target=x86_64-unknown-linux-musl

FROM python:3.9-alpine

# install redis
RUN apk update && \
    apk add --no-cache redis

# install needed python packages
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

WORKDIR /app
COPY --from=builder /usr/local/cargo/bin/pingapi /usr/local/bin/pingapi
COPY background.py start.sh ./

ENV INTERVAL=300
ENV TIMEOUT=10
ENV CONNECTIONS=10
ENV CORS="https://piracy.moe"

EXPOSE 5000
HEALTHCHECK CMD curl --fail http://localhost:5000/health || exit 1

# sed is for replacing windows newline
CMD pingapi && sed -i 's/\r$//' start.sh && sh start.sh