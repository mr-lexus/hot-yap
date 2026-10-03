#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

echo "==> HotYap bootstrap"

echo "==> Checking toolchain"
for tool in node pnpm cargo python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "ERROR: '$tool' is required but not found in PATH."
    case "$tool" in
      node) echo "    Install Node.js (https://nodejs.org) then: npm i -g pnpm" ;;
      pnpm) echo "    Install pnpm: npm i -g pnpm" ;;
      cargo) echo "    Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" ;;
      python3) echo "    Install native Python 3.10+ and ensure the venv module is available." ;;
    esac
    exit 1
  fi
done
if ! python3 -c 'import sys; raise SystemExit(sys.version_info < (3, 10))'; then
  echo "ERROR: Python 3.10 or newer is required."
  exit 1
fi

case "$(uname -s)" in
  Linux)
    echo "==> Tauri Linux system libraries (Ubuntu/Debian/Mint)"
    echo "    Required (needs sudo, one-time):"
    echo "      sudo apt install -y libasound2-dev libgtk-3-dev libwebkit2gtk-4.1-dev \\"
    echo "        libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libssl-dev build-essential pkg-config curl"
    if ! command -v pkg-config >/dev/null 2>&1 \
      || ! pkg-config --exists gtk+-3.0 2>/dev/null \
      || ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
      echo "    WARNING: webkit2gtk/gtk dev packages not detected — 'tauri dev' will fail to compile."
      echo "    Install them with the command above and re-run this script."
    fi
    ;;
  Darwin)
    echo "==> Checking macOS build tools"
    if ! xcode-select -p >/dev/null 2>&1; then
      echo "ERROR: Xcode Command Line Tools are required. Run: xcode-select --install"
      exit 1
    fi
    if [ "$(uname -m)" = "arm64" ]; then
      if [ "$(python3 -c 'import platform; print(platform.machine())')" != "arm64" ]; then
        echo "ERROR: Apple Silicon requires a native arm64 Python; the current Python is running through Rosetta."
        exit 1
      fi
      macos_major="$(sw_vers -productVersion | cut -d. -f1)"
      if [ "$macos_major" -lt 14 ]; then
        echo "ERROR: MLX acceleration on Apple Silicon requires macOS 14 or later."
        exit 1
      fi
    fi
    ;;
esac

echo "==> Python virtual environment"
if [ ! -d backend/.venv ]; then
  python3 -m venv backend/.venv
fi
backend/.venv/bin/pip install --upgrade pip -q
backend/.venv/bin/pip install -r backend/requirements.txt

echo "==> JavaScript dependencies"
pnpm install

echo
echo "==> Done. Run the app with:"
echo "    pnpm tauri dev"
echo
if [ "$(uname -s)" = "Darwin" ]; then
  echo "    Model location: ~/Library/Application Support/com.voxshift.app/models/"
else
  echo "    Model location: ~/.local/share/com.voxshift.app/models/"
fi
