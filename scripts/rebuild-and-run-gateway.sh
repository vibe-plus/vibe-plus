#!/usr/bin/env bash
# Build the vibe CLI, stop any prior instance (pid file), then run the gateway in the foreground.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
PORT="${VIBE_PORT:-15917}"
cargo build -p vibe
"${ROOT}/target/debug/vibe" stop 2>/dev/null || true

# Wait for the port to be released (up to 10 seconds)
for i in $(seq 1 20); do
    if ! lsof -iTCP:"${PORT}" -sTCP:LISTEN -t >/dev/null 2>&1; then
        break
    fi
    if [ "$i" -eq 20 ]; then
        echo "Error: port ${PORT} still in use after 10s, giving up." >&2
        exit 1
    fi
    sleep 0.5
done

exec "${ROOT}/target/debug/vibe" start --foreground --port "${PORT}"
