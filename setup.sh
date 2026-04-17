#!/usr/bin/env bash
set -euo pipefail

# MongoPilot — First-time setup
# Usage: ./setup.sh

echo "=== MongoPilot Setup ==="
echo ""

# ── Prerequisite checks ──────────────────────────────────────────────────────

# Node.js 18+
if ! command -v node >/dev/null 2>&1; then
  echo "Error: Node.js is required (18+). Install from https://nodejs.org"
  exit 1
fi
NODE_MAJOR=$(node -e "process.stdout.write(String(process.versions.node.split('.')[0]))")
if [ "$NODE_MAJOR" -lt 18 ]; then
  echo "Error: Node.js 18+ is required. Found: $(node -v)"
  exit 1
fi
echo "Node.js $(node -v) ... OK"

# pnpm
if ! command -v pnpm >/dev/null 2>&1; then
  echo "Error: pnpm is required. Install with: npm install -g pnpm"
  echo "       or see https://pnpm.io/installation"
  exit 1
fi
echo "pnpm $(pnpm -v) ... OK"

# Rust / cargo (1.70+ required for OnceLock)
if ! command -v rustc >/dev/null 2>&1; then
  echo "Error: Rust (stable, 1.70+) is required."
  echo "       Install from https://rustup.rs"
  exit 1
fi
RUST_VERSION=$(rustc --version | awk '{print $2}')
RUST_MINOR=$(echo "$RUST_VERSION" | cut -d. -f2)
if [ "$RUST_MINOR" -lt 70 ] 2>/dev/null; then
  echo "Warning: Rust 1.70+ is recommended. Found: $RUST_VERSION"
  echo "         Run: rustup update stable"
fi
echo "rustc $RUST_VERSION ... OK"

# Tauri system prerequisites note
echo ""
echo "NOTE: Tauri requires platform-specific system libraries (WebView2 on Windows,"
echo "      webkit2gtk on Linux, Xcode CLI tools on macOS)."
echo "      If this is your first Tauri project, check prerequisites first:"
echo "      https://tauri.app/start/prerequisites/"
echo ""

# ── Install Node dependencies ─────────────────────────────────────────────────

echo "Installing Node dependencies..."
pnpm install

# ── Done ──────────────────────────────────────────────────────────────────────

echo ""
echo "=== Setup complete! ==="
echo ""
echo "Next steps:"
echo "  1. Ensure Tauri system prerequisites are installed for your OS:"
echo "     https://tauri.app/start/prerequisites/"
echo "  2. Start the development build:"
echo "     pnpm tauri dev"
echo "  3. To build a release bundle:"
echo "     pnpm tauri build"
echo ""
echo "Using Claude Code? CLAUDE.md has full project context and all commands."
