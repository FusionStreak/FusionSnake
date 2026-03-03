#!/usr/bin/env bash
set -euo pipefail

echo "=== FusionSnake Trainer ==="
echo "Snake URL:    ${SNAKE_URL:-http://snek:6666}"
echo "Reports:      ${REPORTS_DIR:-/data/reports}"
echo "Trainer port: ${TRAINER_PORT:-5050}"

# Ensure reports directory exists
mkdir -p "${REPORTS_DIR:-/data/reports}"

# Wait for the snake to be reachable before first run
echo "Waiting for snake to be reachable..."
for i in $(seq 1 60); do
    if curl -sf "${SNAKE_URL:-http://snek:6666}/" > /dev/null 2>&1; then
        echo "Snake is up!"
        break
    fi
    if [ "$i" -eq 60 ]; then
        echo "WARNING: Snake not reachable after 60s, proceeding anyway..."
    fi
    sleep 1
done

# Run training immediately on startup (if enough data exists)
echo "Running initial training pass..."
python /app/train.py || echo "Initial training failed (possibly insufficient data)"

# Start the HTTP trigger server — subsequent runs are driven by POST /train
echo "Starting trainer HTTP server on port ${TRAINER_PORT:-5050}..."
exec python /app/server.py
