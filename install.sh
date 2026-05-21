#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# Hakimi Agent Installer
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.sh | bash
#
# Environment variables:
#   HAKIMI_INSTALL_DIR  — Installation directory (default: ~/.hakimi/bin)
#   HAKIMI_VERSION      — Version to install (default: latest)
# ─────────────────────────────────────────────────────────────────────────────

REPO="Mouseww/hakimi-agent"
INSTALL_DIR="${HAKIMI_INSTALL_DIR:-$HOME/.hakimi/bin}"
VERSION="${HAKIMI_VERSION:-latest}"

# ── Color helpers ────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
RESET='\033[0m'

info()    { printf "${BLUE}[INFO]${RESET}  %s\n" "$*"; }
success() { printf "${GREEN}[OK]${RESET}    %s\n" "$*"; }
warn()    { printf "${YELLOW}[WARN]${RESET}  %s\n" "$*"; }
error()   { printf "${RED}[ERR]${RESET}   %s\n" "$*" >&2; }

# ── Pre-flight checks ───────────────────────────────────────────────────────

need_cmd() {
    if ! command -v "$1" &>/dev/null; then
        error "Required command '$1' not found. Please install it and retry."
        exit 1
    fi
}

need_cmd curl
need_cmd tar
need_cmd uname

# ── Detect OS and architecture ───────────────────────────────────────────────

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$ARCH" in
    x86_64)           ARCH="x86_64" ;;
    aarch64|arm64)    ARCH="aarch64" ;;
    *)
        error "Unsupported architecture: $ARCH"
        error "Supported: x86_64, aarch64/arm64"
        exit 1
        ;;
esac

case "$OS" in
    linux)   PLATFORM="unknown-linux-gnu" ;;
    darwin)  PLATFORM="apple-darwin" ;;
    *)
        error "Unsupported OS: $OS"
        error "Supported: linux, darwin (macOS)"
        exit 1
        ;;
esac

info "Detected platform: ${ARCH}-${PLATFORM}"

# ── Determine download URL ──────────────────────────────────────────────────

if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/hakimi-${ARCH}-${PLATFORM}.tar.gz"
else
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/hakimi-${ARCH}-${PLATFORM}.tar.gz"
fi

info "Download URL: ${DOWNLOAD_URL}"

# ── Download and install ─────────────────────────────────────────────────────

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

printf "${BOLD}Installing Hakimi Agent...${RESET}\n"

mkdir -p "$INSTALL_DIR"

info "Downloading binary..."
HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/hakimi.tar.gz" "$DOWNLOAD_URL" 2>/dev/null || true)

if [ "$HTTP_CODE" = "404" ] || [ "$HTTP_CODE" = "403" ] || [ ! -f "$TMPDIR/hakimi.tar.gz" ] || [ ! -s "$TMPDIR/hakimi.tar.gz" ]; then
    warn "No pre-built binary found for ${ARCH}-${PLATFORM} (HTTP ${HTTP_CODE:-unknown})."
    warn ""

    # ── Fallback: build from source with cargo ────────────────────────────
    if command -v cargo &>/dev/null; then
        info "Falling back to building from source via cargo..."
        info "This may take a few minutes."

        BUILD_DIR=$(mktemp -d)
        trap 'rm -rf "$TMPDIR" "$BUILD_DIR"' EXIT

        info "Cloning repository..."
        if command -v git &>/dev/null; then
            git clone --depth 1 "https://github.com/${REPO}.git" "$BUILD_DIR/hakimi-agent" 2>/dev/null
        else
            need_cmd unzip
            curl -sSL "https://github.com/${REPO}/archive/refs/heads/main.zip" -o "$BUILD_DIR/src.zip"
            unzip -q "$BUILD_DIR/src.zip" -d "$BUILD_DIR"
            mv "$BUILD_DIR/hakimi-agent-main" "$BUILD_DIR/hakimi-agent"
        fi

        info "Building hakimi-agent (release mode)..."
        (
            cd "$BUILD_DIR/hakimi-agent"
            cargo build --release -p hakimi-cli 2>&1 | tail -5
        )

        if [ -f "$BUILD_DIR/hakimi-agent/target/release/hakimi" ]; then
            cp "$BUILD_DIR/hakimi-agent/target/release/hakimi" "$INSTALL_DIR/hakimi"
            chmod +x "$INSTALL_DIR/hakimi"
            success "Built from source and installed."
        else
            error "Build failed. Binary not found at expected path."
            exit 1
        fi
    else
        error "cargo not found. Cannot build from source."
        error ""
        error "Options:"
        error "  1. Install Rust: https://rustup.rs"
        error "  2. Download a release manually from:"
        error "     https://github.com/${REPO}/releases"
        exit 1
    fi
else
    info "Extracting binary..."
    tar xzf "$TMPDIR/hakimi.tar.gz" -C "$TMPDIR"

    if [ ! -f "$TMPDIR/hakimi" ]; then
        error "Archive did not contain 'hakimi' binary."
        exit 1
    fi

    cp "$TMPDIR/hakimi" "$INSTALL_DIR/hakimi"
    chmod +x "$INSTALL_DIR/hakimi"
    success "Binary installed."
fi

# ── PATH setup ───────────────────────────────────────────────────────────────

echo ""
info "Installed to: ${INSTALL_DIR}/hakimi"
echo ""

# Check if already in PATH
if echo "$PATH" | tr ':' '\n' | grep -qxF "$INSTALL_DIR"; then
    success "Install directory is already in your PATH."
else
    echo "  Add to your PATH:"
    echo ""
    printf "    ${BOLD}export PATH=\"%s:\$PATH\"${RESET}\n" "$INSTALL_DIR"
    echo ""

    # Try to detect shell profile and offer to add it
    SHELL_RC=""
    case "$(basename "${SHELL:-/bin/bash}")" in
        bash)  SHELL_RC="${HOME}/.bashrc" ;;
        zsh)   SHELL_RC="${HOME}/.zshrc" ;;
        fish)  SHELL_RC="${HOME}/.config/fish/config.fish" ;;
        *)     SHELL_RC="" ;;
    esac

    if [ -n "$SHELL_RC" ] && [ -f "$SHELL_RC" ]; then
        if ! grep -q '.hakimi/bin' "$SHELL_RC" 2>/dev/null; then
            # In non-interactive (piped) mode, skip the prompt and just print instructions
            if [ -t 0 ]; then
                read -rp "  Add to ${SHELL_RC}? [Y/n] " answer
                answer="${answer:-Y}"
                if [ "$answer" != "n" ] && [ "$answer" != "N" ]; then
                    if [ "$(basename "$SHELL")" = "fish" ]; then
                        echo 'set -gx PATH $HOME/.hakimi/bin $PATH' >> "$SHELL_RC"
                    else
                        echo 'export PATH="$HOME/.hakimi/bin:$PATH"' >> "$SHELL_RC"
                    fi
                    success "Added to ${SHELL_RC}. Run 'source ${SHELL_RC}' or open a new terminal."
                fi
            else
                echo "  For non-interactive install, add this to ${SHELL_RC}:"
                echo ""
                if [ "$(basename "${SHELL:-/bin/bash}")" = "fish" ]; then
                    echo "    set -gx PATH \$HOME/.hakimi/bin \$PATH"
                else
                    echo "    export PATH=\"\$HOME/.hakimi/bin:\$PATH\""
                fi
            fi
        else
            success "PATH entry already exists in ${SHELL_RC}."
        fi
    fi
fi

# ── Verify installation ─────────────────────────────────────────────────────

echo ""
if "$INSTALL_DIR/hakimi" --version &>/dev/null; then
    success "Hakimi Agent installed successfully!"
    echo ""
    printf "  Run ${BOLD}hakimi${RESET} to get started.\n"
else
    success "Hakimi Agent installed to ${INSTALL_DIR}/hakimi"
fi

# ── Optional: run setup wizard ──────────────────────────────────────────────

echo ""
if [ -t 0 ]; then
    read -rp "Run setup wizard now? [Y/n] " answer
    answer="${answer:-Y}"
    if [ "$answer" != "n" ] && [ "$answer" != "N" ]; then
        "$INSTALL_DIR/hakimi" --help || true
    fi
else
    info "Run '$INSTALL_DIR/hakimi --help' to get started."
fi
