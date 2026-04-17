#!/usr/bin/env bash
# preview-docs.sh — generate and serve the themed API explorer locally.
#
# Opens http://localhost:8787 in the default browser once the server is ready.
# Press Ctrl-C to stop.
#
# Usage:
#   ./scripts/preview-docs.sh             # primary spec (all routes)
#   ./scripts/preview-docs.sh --readonly  # read-only spec

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREVIEW_DIR="${TMPDIR:-/tmp}/stophammer-docs-preview"
PORT=8787
SPEC_FLAG="${1:-}"

echo "→ building gen_openapi…"
cargo build --bin gen_openapi --manifest-path "$REPO_ROOT/Cargo.toml" --quiet

echo "→ generating openapi.json…"
mkdir -p "$PREVIEW_DIR"
# shellcheck disable=SC2086
"$REPO_ROOT/target/debug/gen_openapi" $SPEC_FLAG > "$PREVIEW_DIR/openapi.json"

echo "→ copying api.html…"
cp "$REPO_ROOT/api.html" "$PREVIEW_DIR/index.html"

echo "→ starting HTTP server on http://localhost:$PORT"
echo "   Press Ctrl-C to stop."

# open the browser after a short delay so the server is ready
(sleep 1 && open "http://localhost:$PORT" 2>/dev/null || xdg-open "http://localhost:$PORT" 2>/dev/null || true) &

cd "$PREVIEW_DIR"
exec python3 -m http.server "$PORT"
