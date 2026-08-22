#!/bin/sh
# Portly installer — downloads the latest release binary for your platform.
#
#   curl -fsSL https://raw.githubusercontent.com/sambai-dev/portly/main/install.sh | sh
#
# Installs to PORTLY_INSTALL_DIR (default: ~/.local/bin if writable,
# otherwise /usr/local/bin with sudo).

set -eu

REPO="sambai-dev/portly"
BIN_NAME="portly"

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

# --- resolve OS/arch to the Rust target triple used by release CI -----------
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux) case "$ARCH" in
        x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
        aarch64 | arm64) die "linux-aarch64 builds are not published yet; build from source: cargo install portly" ;;
        *) die "unsupported arch: $ARCH" ;;
    esac ;;
    Darwin) case "$ARCH" in
        arm64 | aarch64) TARGET="aarch64-apple-darwin" ;;
        x86_64) TARGET="x86_64-apple-darwin" ;;
        *) die "unsupported mac arch: $ARCH" ;;
    esac ;;
    *) die "unsupported OS: $OSTYPE (on Windows use install.ps1)" ;;
esac

ASSET="${BIN_NAME}-${TARGET}.tar.gz"

# --- pick an install dir -----------------------------------------------------
if [ -n "${PORTLY_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$PORTLY_INSTALL_DIR"
elif [ -w "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    INSTALL_DIR="$HOME/.local/bin"
else
    INSTALL_DIR="/usr/local/bin"
    SUDO="sudo"
fi

# --- fetch latest release asset ----------------------------------------------
TMPDIR_PORTLY="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_PORTLY"' EXIT

URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
log "downloading ${URL}"
curl -fSL --retry 3 -o "$TMPDIR_PORTLY/$ASSET" "$URL" || die "download failed"

tar -xzf "$TMPDIR_PORTLY/$ASSET" -C "$TMPDIR_PORTLY" || die "extract failed"

[ -f "$TMPDIR_PORTLY/$BIN_NAME" ] || die "archive did not contain $BIN_NAME"

if [ -n "${SUDO:-}" ]; then
    $SUDO install -m 0755 "$TMPDIR_PORTLY/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
else
    install -m 0755 "$TMPDIR_PORTLY/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
fi

log "installed $INSTALL_DIR/$BIN_NAME"
"$INSTALL_DIR/$BIN_NAME" --version || true
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) log "note: add $INSTALL_DIR to your PATH" ;;
esac
