ARG RUST_VERSION=1.78.0

FROM rust:${RUST_VERSION}-slim-bookworm as builder
WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp ./target/release/vercel-log-drain /vercel-log-drain

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /vercel-log-drain /usr/local/bin/vercel-log-drain
ENTRYPOINT [ "vercel-log-drain" ]
