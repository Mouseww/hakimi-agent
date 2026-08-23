#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

export PATH="/usr/bin:/usr/sbin:/bin:/sbin:/usr/local/bin:/usr/local/sbin:/root/.cargo/bin:$PATH"

echo "==> Release smoke: gateway streaming regression tests"
cargo test -p hakimi-cli gateway_ -- --nocapture

echo "==> Release smoke: TUI launcher routing tests"
cargo test -p hakimi-cli tui_ -- --nocapture

echo "==> Release smoke: format check"
cargo fmt --all -- --check

echo "==> Release smoke: hakimi-agent no-default-features tests"
cargo test -p hakimi-agent --no-default-features

echo "==> Release smoke: TUI package tests"
cargo test -p hakimi-tui parse_tui_startup_command -- --nocapture

if [[ -n "${GITHUB_REF_NAME:-}" ]]; then
  expected_version="${GITHUB_REF_NAME#v}"
  cargo_version="$({ cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "hakimi-agent") | .version'; } 2>/dev/null || true)"

  if [[ -z "$cargo_version" ]]; then
    cargo_version="$(cargo pkgid -p hakimi-agent | sed -E 's/.*#([0-9][^#]*)$/\1/')"
  fi

  if [[ "$cargo_version" != "$expected_version" ]]; then
    echo "Version mismatch: tag ${GITHUB_REF_NAME} expects ${expected_version}, Cargo has ${cargo_version}" >&2
    exit 1
  fi

  echo "==> Release smoke: tag version matches Cargo.toml (${cargo_version})"
fi

echo "==> Release smoke: OK"
