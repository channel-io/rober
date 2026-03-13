#!/usr/bin/env bash
set -euo pipefail

PORT="${GATEWAY_PORT:-8090}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

cleanup() {
    echo ""
    echo "Shutting down..."
    kill $GATEWAY_PID $TUNNEL_PID 2>/dev/null || true
    wait $GATEWAY_PID $TUNNEL_PID 2>/dev/null || true
}
trap cleanup EXIT

echo "Building zeroclaw..."
(cd zeroclaw && cargo build --release) 2>&1

echo "Starting zeroclaw gateway on port $PORT..."
./zeroclaw/target/release/zeroclaw gateway start --host 127.0.0.1 --port "$PORT" &
GATEWAY_PID=$!

sleep 2
if ! kill -0 $GATEWAY_PID 2>/dev/null; then
    echo "Gateway failed to start"
    exit 1
fi

echo "Starting cloudflared tunnel..."
cloudflared tunnel --url "http://127.0.0.1:$PORT" &
TUNNEL_PID=$!

echo ""
echo "Gateway running (PID $GATEWAY_PID)"
echo "Tunnel  running (PID $TUNNEL_PID)"
echo "Press Ctrl+C to stop"

wait
