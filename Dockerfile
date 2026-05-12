# syntax=docker/dockerfile:1.7
# Build a fully static, scratch-based holdon image.
#
#   docker build -t holdon .
#   docker run --rm holdon postgres://db:5432 https://api/health

FROM rust:1.85-alpine AS builder
ARG TARGETARCH
RUN apk add --no-cache musl-dev pkgconfig
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
COPY examples ./examples
COPY README.md LICENSE-MIT LICENSE-APACHE ./
RUN case "${TARGETARCH}" in \
    amd64) target=x86_64-unknown-linux-musl ;; \
    arm64) target=aarch64-unknown-linux-musl ;; \
    *) echo "unsupported TARGETARCH=${TARGETARCH}" >&2 ; exit 1 ;; \
    esac \
 && rustup target add "${target}" \
 && cargo build --release --locked --all-features --target "${target}" \
 && strip "/src/target/${target}/release/holdon" \
 && cp "/src/target/${target}/release/holdon" /holdon

FROM scratch
COPY --from=builder /holdon /holdon
USER 65532:65532
ENTRYPOINT ["/holdon"]

LABEL org.opencontainers.image.title="holdon"
LABEL org.opencontainers.image.description="Wait for anything. Know why if it doesn't."
LABEL org.opencontainers.image.licenses="MIT OR Apache-2.0"
LABEL org.opencontainers.image.source="https://github.com/imjustprism/holdon"
