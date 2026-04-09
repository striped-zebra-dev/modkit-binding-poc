#!/bin/bash
set -e

cd "$(dirname "$0")"

cleanup() {
    [ -n "$SPI_PID" ] && kill "$SPI_PID" 2>/dev/null
    [ -n "$API_PID" ] && kill "$API_PID" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

cargo build --quiet 2>/dev/null

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  PART 1: Compile-time SPI (email plugin, zero-cost)    ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
./target/debug/poc-host

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  PART 2: REST SPI (SMS gateway) + REST API             ║"
echo "║  Directory validates OpenAPI specs on registration      ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

PORT=4001 ./target/debug/notification-plugin-remote &
SPI_PID=$!

PORT=4010 ./target/debug/notification &
API_PID=$!

# Wait for both servers to be ready
for port in 4001 4010; do
    for i in $(seq 1 20); do
        if curl -sf "http://localhost:$port/.well-known/openapi.json" > /dev/null 2>&1; then
            break
        fi
        sleep 0.2
    done
done

BINDING_MODE=rest \
  NOTIFICATION_SPI_URL=http://localhost:4001 \
  NOTIFICATION_API_URL=http://localhost:4010 \
  ./target/debug/poc-host
