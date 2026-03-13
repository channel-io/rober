#!/usr/bin/env bash
set -euo pipefail

PORT="${GATEWAY_PORT:-8090}"
CONFIG="${GATEWAY_CONFIG:-configs/gateway/config.toml}"

cleanup() {
    echo ""
    echo "Shutting down..."
    kill $GATEWAY_PID $TUNNEL_PID 2>/dev/null || true
    wait $GATEWAY_PID $TUNNEL_PID 2>/dev/null || true
}
trap cleanup EXIT

echo "Building gateway..."
cargo build --release -p rover-gateway 2>&1

echo "Starting gateway on port $PORT..."
GATEWAY_CONFIG="$CONFIG" ./target/release/rover-gateway &
GATEWAY_PID=$!

sleep 1
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
