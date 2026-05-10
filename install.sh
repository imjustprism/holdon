#!/usr/bin/env sh
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/imjustprism/holdon/main/install.sh | sh
#   curl -fsSL .../install.sh | HOLDON_VERSION=v0.1.0 INSTALL_DIR=$HOME/bin sh

set -eu

REPO="imjustprism/holdon"
VERSION="${HOLDON_VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

need() { command -v "$1" >/dev/null 2>&1 || { echo "install: missing required tool: $1" >&2; exit 1; }; }
need curl
need tar
need uname
need sha256sum || need shasum

detect_target() {
    os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    arch="$(uname -m)"
    case "$os" in
        linux)
            case "$arch" in
                x86_64|amd64) echo "x86_64-unknown-linux-musl" ;;
                aarch64|arm64) echo "aarch64-unknown-linux-musl" ;;
                *) echo "install: unsupported arch: $arch" >&2; exit 1 ;;
            esac ;;
        darwin)
            case "$arch" in
                x86_64) echo "x86_64-apple-darwin" ;;
                arm64|aarch64) echo "aarch64-apple-darwin" ;;
                *) echo "install: unsupported arch: $arch" >&2; exit 1 ;;
            esac ;;
        *) echo "install: unsupported OS: $os (use install.ps1 on Windows)" >&2; exit 1 ;;
    esac
}

resolve_version() {
    if [ "$VERSION" = "latest" ]; then
        VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
        [ -n "$VERSION" ] || { echo "install: could not resolve latest version" >&2; exit 1; }
    fi
}

verify_checksum() {
    file="$1"
    expected="$2"
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$file" | awk '{print $1}')"
    else
        actual="$(shasum -a 256 "$file" | awk '{print $1}')"
    fi
    [ "$actual" = "$expected" ] || { echo "install: checksum mismatch for $file" >&2; exit 1; }
}

main() {
    target="$(detect_target)"
    resolve_version
    archive="holdon-${target}.tar.gz"
    base="https://github.com/${REPO}/releases/download/${VERSION}"

    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    echo "install: fetching holdon ${VERSION} for ${target}"
    curl -fsSL -o "${tmp}/${archive}" "${base}/${archive}"
    curl -fsSL -o "${tmp}/SHA256SUMS" "${base}/SHA256SUMS"

    expected="$(grep " ${archive}\$" "${tmp}/SHA256SUMS" | awk '{print $1}')"
    [ -n "$expected" ] || { echo "install: ${archive} missing from SHA256SUMS" >&2; exit 1; }
    verify_checksum "${tmp}/${archive}" "$expected"

    tar -xzf "${tmp}/${archive}" -C "${tmp}"
    mkdir -p "${INSTALL_DIR}"
    install -m 0755 "${tmp}/holdon-${target}/holdon" "${INSTALL_DIR}/holdon"

    echo "install: holdon installed to ${INSTALL_DIR}/holdon"
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;
        *) echo "install: add ${INSTALL_DIR} to PATH" ;;
    esac
}

main "$@"
