#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

echo "==> VoxShift bootstrap"

echo "==> Checking toolchain"
for tool in node pnpm cargo python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "ERROR: '$tool' is required but not found in PATH."
    case "$tool" in
      node) echo "    Install Node.js (https://nodejs.org) then: npm i -g pnpm" ;;
      pnpm) echo "    Install pnpm: npm i -g pnpm" ;;
      cargo) echo "    Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" ;;
      python3) echo "    Install Python 3.10+ (apt install python3 python3-venv)" ;;
    esac
    exit 1
  fi
done

echo "==> Tauri Linux system libraries (Ubuntu/Debian/Mint)"
echo "    Required (needs sudo, one-time):"
echo "      sudo apt install -y libasound2-dev libgtk-3-dev libwebkit2gtk-4.1-dev \\"
echo "        libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libssl-dev build-essential pkg-config curl"
if ! pkg-config --exists gtk+-3.0 2>/dev/null || ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
  echo "    WARNING: webkit2gtk/gtk dev packages not detected — 'tauri dev' will fail to compile."
  echo "    Install them with the command above and re-run this script."
fi

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
echo "    Model location: ~/.local/share/com.voxshift.app/models/ru-en-codeswitch/"
