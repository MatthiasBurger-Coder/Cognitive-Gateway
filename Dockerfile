FROM rust:1.98.0-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build -j 1 --release --locked -p gateway-daemon --bin cg --bin cg-registry --example local-model-proposal

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/cg /build/target/release/cg-registry /usr/local/bin/
COPY --from=builder /build/target/release/examples/local-model-proposal /usr/local/bin/
COPY catalog /app/catalog
WORKDIR /app
ENTRYPOINT ["cg"]
CMD ["--help"]
