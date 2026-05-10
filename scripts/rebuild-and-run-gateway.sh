#!/usr/bin/env bash
# Build the vibe CLI, stop any prior instance (pid file), then run the gateway in the foreground.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PORT="${VIBE_PORT:-15917}"
cargo build -p vibe
"${ROOT}/target/debug/vibe" stop 2>/dev/null || true
exec "${ROOT}/target/debug/vibe" start --foreground --port "${PORT}"
