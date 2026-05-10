# syntax=docker/dockerfile:1.7
# Build a fully static, scratch-based holdon image.
#
#   docker build -t holdon .
#   docker run --rm holdon postgres://db:5432 https://api/health

FROM rust:1.85-alpine AS builder
RUN apk add --no-cache musl-dev pkgconfig
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples
COPY README.md LICENSE-MIT LICENSE-APACHE ./
RUN cargo build --release --locked --all-features --target x86_64-unknown-linux-musl \
 && strip /src/target/x86_64-unknown-linux-musl/release/holdon

FROM scratch
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/holdon /holdon
USER 65532:65532
ENTRYPOINT ["/holdon"]

LABEL org.opencontainers.image.title="holdon"
LABEL org.opencontainers.image.description="Wait for anything. Know why if it doesn't."
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"
LABEL org.opencontainers.image.source="https://github.com/imjustprism/holdon"
