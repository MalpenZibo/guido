#!/bin/bash
# Run visual regression tests locally using Docker
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

echo "Building and running visual tests..."
docker compose run --rm visual-tests
