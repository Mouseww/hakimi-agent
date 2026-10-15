#!/usr/bin/env bash
# Correct Daily Maintenance security-scan step.
# Prefer calling this from .github/workflows/daily-maintenance.yml once a
# token with the `workflow` scope can update that file.
set -euo pipefail

cargo install cargo-audit --locked 2>/dev/null || cargo install cargo-audit || true

if ! command -v jq >/dev/null 2>&1; then
  if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update -qq && sudo apt-get install -y -qq jq >/dev/null
  else
    echo "jq is required to count cargo-audit vulnerabilities" >&2
    exit 1
  fi
fi

cargo audit --json > audit.json || true
count="$(jq -r '.vulnerabilities.count // (.vulnerabilities.list | length) // 0' audit.json)"
echo "Vulnerability count (after ignores): $count"

summary_target="${GITHUB_STEP_SUMMARY:-/dev/stdout}"
{
  echo "## 🔐 安全漏洞扫描"
  echo ""
  if [ "$count" -gt 0 ]; then
    echo "### ⚠️ 发现 $count 个安全漏洞"
  else
    echo "### ✅ 无未忽略的安全漏洞"
  fi
  echo ""
  echo '```'
  cargo audit 2>&1 || true
  echo '```'
} >> "$summary_target"

if [ "$count" -gt 0 ]; then
  echo "Failing: $count vulnerability(ies) remain after ignores" >&2
  exit 1
fi
