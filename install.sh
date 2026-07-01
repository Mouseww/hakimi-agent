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
DEFAULT_INSTALL_DIR="$HOME/.hakimi/bin"
INSTALL_DIR="${HAKIMI_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
INSTALL_DIR="${INSTALL_DIR%/}"
VERSION="${HAKIMI_VERSION:-latest}"
SYSTEM_BIN_DIR="/usr/local/bin"
SYSTEM_HAKIMI="$SYSTEM_BIN_DIR/hakimi"
WANT_SYSTEM_LINK=0

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
    linux)
        # Prefer musl (statically linked, works on any Linux)
        PLATFORM="unknown-linux-musl"
        FALLBACK_PLATFORM="unknown-linux-gnu"
        WANT_SYSTEM_LINK=1
        ;;
    darwin)  PLATFORM="apple-darwin" ;;
    mingw*|msys*|cygwin*)
        error "Windows detected. Please use the PowerShell installer instead:"
        error ""
        error "  irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex"
        error ""
        error "Or install via cargo:"
        error "  cargo install hakimi-agent"
        exit 1
        ;;
    *)
        error "Unsupported OS: $OS"
        error "Supported: linux, darwin (macOS)"
        error ""
        error "For Windows, use the PowerShell installer:"
        error "  irm https://raw.githubusercontent.com/Mouseww/hakimi-agent/main/install.ps1 | iex"
        error ""
        error "Or install via cargo:"
        error "  cargo install hakimi-agent"
        exit 1
        ;;
esac

info "Detected platform: ${ARCH}-${PLATFORM}"

if [ "$INSTALL_DIR" = "$SYSTEM_BIN_DIR" ]; then
    warn "Refusing to install the real binary directly into ${SYSTEM_BIN_DIR}."
    warn "${SYSTEM_HAKIMI} is reserved for a symlink/launcher only."
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
    WANT_SYSTEM_LINK=1
    info "Using managed install directory instead: ${INSTALL_DIR}"
fi

# ── Determine download URL ──────────────────────────────────────────────────

if [ "$VERSION" = "latest" ]; then
    DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/hakimi-${ARCH}-${PLATFORM}.tar.gz"
else
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/hakimi-${ARCH}-${PLATFORM}.tar.gz"
fi

# Fallback URL for Linux (musl → gnu)
FALLBACK_URL=""
if [ -n "${FALLBACK_PLATFORM:-}" ]; then
    if [ "$VERSION" = "latest" ]; then
        FALLBACK_URL="https://github.com/${REPO}/releases/latest/download/hakimi-${ARCH}-${FALLBACK_PLATFORM}.tar.gz"
    else
        FALLBACK_URL="https://github.com/${REPO}/releases/download/${VERSION}/hakimi-${ARCH}-${FALLBACK_PLATFORM}.tar.gz"
    fi
fi

info "Download URL: ${DOWNLOAD_URL}"

# ── Download and install ─────────────────────────────────────────────────────

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Print banner
echo ""
echo "${BOLD}${BLUE}╔══════════════════════════════════════════════════════════════╗${RESET}"
echo "${BOLD}${BLUE}║                                                              ║${RESET}"
echo "${BOLD}${BLUE}║${RESET}   ${BOLD}${GREEN}_  _               _ _             _${RESET}                   ${BOLD}${BLUE}║${RESET}"
echo "${BOLD}${BLUE}║${RESET}  ${BOLD}${GREEN}| || |__ _ __ _ __ (_) |_ _  _ __ _| |___ _ _${RESET}          ${BOLD}${BLUE}║${RESET}"
echo "${BOLD}${BLUE}║${RESET}  ${BOLD}${GREEN}| __ / _\` / _| '  \\| |  _| || / _\` | / -_) '_|${RESET}         ${BOLD}${BLUE}║${RESET}"
echo "${BOLD}${BLUE}║${RESET}  ${BOLD}${GREEN}|_||_\\__,_\\__|_|_|_|_|\\__|\\_, \\__,_|_\\___|_|${RESET}           ${BOLD}${BLUE}║${RESET}"
echo "${BOLD}${BLUE}║${RESET}                         ${BOLD}${GREEN}|__/${RESET}                            ${BOLD}${BLUE}║${RESET}"
echo "${BOLD}${BLUE}║                                                              ║${RESET}"
echo "${BOLD}${BLUE}║${RESET}            ${BOLD}${YELLOW}AI-Powered Development Environment${RESET}            ${BOLD}${BLUE}║${RESET}"
echo "${BOLD}${BLUE}║                                                              ║${RESET}"
echo "${BOLD}${BLUE}╚══════════════════════════════════════════════════════════════╝${RESET}"
echo ""

mkdir -p "$INSTALL_DIR"

info "Downloading binary..."
HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/hakimi.tar.gz" "$DOWNLOAD_URL" 2>/dev/null || true)

# Try fallback URL if primary failed (musl → gnu on Linux)
if [ "$HTTP_CODE" = "404" ] && [ -n "${FALLBACK_URL:-}" ]; then
    info "Trying fallback: ${FALLBACK_URL}"
    HTTP_CODE=$(curl -sSL -w '%{http_code}' -o "$TMPDIR/hakimi.tar.gz" "$FALLBACK_URL" 2>/dev/null || true)
fi

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
            cargo build --release -p hakimi-agent 2>&1 | tail -5
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

link_hakimi_into() {
    local link_dir="$1"
    local link_path="${link_dir}/hakimi"

    if [ "$link_path" = "${INSTALL_DIR}/hakimi" ]; then
        return 0
    fi

    if ln -sfn "${INSTALL_DIR}/hakimi" "$link_path"; then
        success "Linked hakimi into ${link_dir}."
        return 0
    fi

    return 1
}

ensure_usr_local_hakimi_is_shim() {
    local should_create="${1:-0}"

    if [ ! -d "$SYSTEM_BIN_DIR" ]; then
        return 0
    fi

    if [ ! -L "$SYSTEM_HAKIMI" ] && [ ! -e "$SYSTEM_HAKIMI" ] && [ "$should_create" != "1" ]; then
        return 0
    fi

    if [ -L "$SYSTEM_HAKIMI" ] || [ -e "$SYSTEM_HAKIMI" ]; then
        if [ -L "$SYSTEM_HAKIMI" ]; then
            info "Refreshing ${SYSTEM_HAKIMI} symlink."
        else
            warn "${SYSTEM_HAKIMI} exists as a regular file; replacing it with a symlink."
        fi
    fi

    if [ -w "$SYSTEM_BIN_DIR" ]; then
        link_hakimi_into "$SYSTEM_BIN_DIR" || true
    elif command -v sudo &>/dev/null && sudo -n true 2>/dev/null; then
        if sudo ln -sfn "${INSTALL_DIR}/hakimi" "$SYSTEM_HAKIMI"; then
            success "Linked hakimi into ${SYSTEM_BIN_DIR}."
        fi
    elif [ "$should_create" = "1" ] || [ -e "$SYSTEM_HAKIMI" ]; then
        warn "Could not create ${SYSTEM_HAKIMI} automatically."
        warn "Run: sudo ln -sfn \"${INSTALL_DIR}/hakimi\" \"${SYSTEM_HAKIMI}\""
    fi
}

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

    # Try to detect shell profile and add it automatically. The README install
    # command is usually a curl|bash pipeline, so stdin is not a TTY; prompting
    # here leaves most users without a working `hakimi` command.
    SHELL_RC=""
    case "$(basename "${SHELL:-/bin/bash}")" in
        bash)  SHELL_RC="${HOME}/.bashrc" ;;
        zsh)   SHELL_RC="${HOME}/.zshrc" ;;
        fish)  SHELL_RC="${HOME}/.config/fish/config.fish" ;;
        *)     SHELL_RC="" ;;
    esac

    if [ -n "$SHELL_RC" ]; then
        mkdir -p "$(dirname "$SHELL_RC")"
        touch "$SHELL_RC"
        if ! grep -q '.hakimi/bin' "$SHELL_RC" 2>/dev/null; then
            if [ "$(basename "${SHELL:-/bin/bash}")" = "fish" ]; then
                echo 'set -gx PATH $HOME/.hakimi/bin $PATH' >> "$SHELL_RC"
            else
                echo 'export PATH="$HOME/.hakimi/bin:$PATH"' >> "$SHELL_RC"
            fi
            success "Added ${INSTALL_DIR} to ${SHELL_RC}."
            warn "Run 'source ${SHELL_RC}' or open a new terminal before typing 'hakimi'."
        else
            success "PATH entry already exists in ${SHELL_RC}."
        fi
    fi

    if [ -d "$HOME/.local/bin" ] && echo "$PATH" | tr ':' '\n' | grep -qxF "$HOME/.local/bin"; then
        link_hakimi_into "$HOME/.local/bin" || true
    else
        WANT_SYSTEM_LINK=1
    fi
fi

ensure_usr_local_hakimi_is_shim "$WANT_SYSTEM_LINK"

# ── Verify installation ─────────────────────────────────────────────────────

echo ""
echo "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
if "$INSTALL_DIR/hakimi" --version &>/dev/null; then
    VERSION_INFO=$("$INSTALL_DIR/hakimi" --version 2>&1 | head -1)
    echo ""
    echo "  ${GREEN}✓${RESET} ${BOLD}${GREEN}Installation successful!${RESET}"
    echo ""
    echo "  ${BOLD}Version:${RESET}        ${VERSION_INFO}"
    echo "  ${BOLD}Installed to:${RESET}   ${INSTALL_DIR}/hakimi"
    echo ""
else
    echo "  ${GREEN}✓${RESET} ${BOLD}Hakimi Agent installed to ${INSTALL_DIR}/hakimi${RESET}"
fi
echo "${BOLD}${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"

# ── Optional: run setup wizard ──────────────────────────────────────────────

echo ""
if [ -t 0 ]; then
    printf "${BOLD}Run setup wizard now to configure Hakimi?${RESET} [Y/n] "
    read -r answer
    answer="${answer:-Y}"
    if [ "$answer" != "n" ] && [ "$answer" != "N" ]; then
        echo ""
        "$INSTALL_DIR/hakimi" setup || true
    else
        echo ""
        info "You can run 'hakimi setup' anytime to configure your model/API key."
    fi
else
    info "Run 'hakimi setup' to configure your model/API key."
fi
