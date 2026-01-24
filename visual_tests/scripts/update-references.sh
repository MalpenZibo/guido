#!/bin/bash
# Update reference images for visual regression tests
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

echo "Updating reference images..."
docker compose run --rm update-references

echo ""
echo "Reference images updated. Don't forget to commit them:"
echo "  git add visual_tests/references/"
echo "  git commit -m 'Update visual test reference images'"
