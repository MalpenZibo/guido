#!/bin/bash
set -e

# Ensure runtime directory exists
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"

# Fix ownership of workspace directories if needed (using sudo)
if [ -d /workspace/target ] && [ ! -w /workspace/target ]; then
    echo "Fixing target directory permissions..."
    sudo chown -R testuser:testuser /workspace/target
fi

# Ensure target directory exists and is writable
mkdir -p /workspace/target 2>/dev/null || sudo mkdir -p /workspace/target
sudo chown -R testuser:testuser /workspace/target 2>/dev/null || true

# Also fix visual_tests directories
mkdir -p /workspace/visual_tests/references 2>/dev/null || true
mkdir -p /workspace/visual_tests/output 2>/dev/null || true

# Start Sway in background
echo "Starting Sway..."
sway &
SWAY_PID=$!

# Wait for Wayland socket to be available
echo "Waiting for Wayland socket..."
TIMEOUT=30
ELAPSED=0
while [ ! -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; do
    sleep 0.1
    ELAPSED=$((ELAPSED + 1))
    if [ $ELAPSED -ge $((TIMEOUT * 10)) ]; then
        echo "Timeout waiting for Wayland socket"
        exit 1
    fi
done
echo "Wayland socket ready"

# Give Sway a moment to fully initialize
sleep 0.5

# Run the command
echo "Executing: $@"
"$@"
EXIT_CODE=$?

# Cleanup
echo "Cleaning up..."
kill $SWAY_PID 2>/dev/null || true
wait $SWAY_PID 2>/dev/null || true

exit $EXIT_CODE
