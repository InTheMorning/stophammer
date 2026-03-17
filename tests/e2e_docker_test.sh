#!/usr/bin/env bash
set -euo pipefail

# ── E2E Docker Security Smoke Tests ─────────────────────────────────────────
#
# Brings up the stophammer E2E security compose environment, runs a suite of
# HTTP smoke tests against the primary node, and tears everything down.
#
# Prerequisites:
#   - docker and docker compose v2 installed
#   - Run from the repository root (parent of stophammer/)
#
# Usage:
#   ./stophammer/tests/e2e_docker_test.sh

COMPOSE_FILE="docker-compose.e2e-security.yml"
PRIMARY_URL="http://localhost:8008"
ADMIN_TOKEN="test-admin-token-e2e"

PASSED=0
FAILED=0
TESTS=()

# ── Helpers ─────────────────────────────────────────────────────────────────

log()  { printf '\033[1;34m[e2e]\033[0m %s\n' "$*"; }
pass() { PASSED=$((PASSED + 1)); TESTS+=("PASS: $1"); printf '\033[1;32m  PASS\033[0m %s\n' "$1"; }
fail() { FAILED=$((FAILED + 1)); TESTS+=("FAIL: $1"); printf '\033[1;31m  FAIL\033[0m %s\n' "$1"; }

# Navigate to repo root (parent of stophammer/).
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

cleanup() {
    log "Tearing down E2E environment..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
}

# Always tear down on exit, even on failure.
trap cleanup EXIT

# ── Bring up the environment ────────────────────────────────────────────────

log "Building and starting E2E environment..."
docker compose -f "$COMPOSE_FILE" up -d --build --wait 2>&1

# ── Wait for primary to be healthy ─────────────────────────────────────────

log "Waiting for primary to become healthy..."
MAX_RETRIES=30
RETRY_INTERVAL=2

for i in $(seq 1 "$MAX_RETRIES"); do
    if curl -sf "${PRIMARY_URL}/health" > /dev/null 2>&1; then
        log "Primary is healthy (attempt $i/$MAX_RETRIES)."
        break
    fi
    if [ "$i" -eq "$MAX_RETRIES" ]; then
        log "Primary did not become healthy after $MAX_RETRIES attempts."
        docker compose -f "$COMPOSE_FILE" logs primary
        exit 1
    fi
    sleep "$RETRY_INTERVAL"
done

# ── Smoke Tests ─────────────────────────────────────────────────────────────

log "Running smoke tests against ${PRIMARY_URL}..."

# --------------------------------------------------------------------------
# Test 1: GET /health returns 200
# --------------------------------------------------------------------------
test_name="GET /health returns 200"
status=$(curl -s -o /dev/null -w '%{http_code}' "${PRIMARY_URL}/health")
if [ "$status" = "200" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 2: GET /v1/recent returns 200
# The API has no /v1/feeds list endpoint; /v1/recent is the closest read-only
# listing endpoint available on both primary and community nodes.
# --------------------------------------------------------------------------
test_name="GET /v1/recent returns 200"
status=$(curl -s -o /dev/null -w '%{http_code}' "${PRIMARY_URL}/v1/recent")
if [ "$status" = "200" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 3: GET /node/info returns 200 with pubkey
# --------------------------------------------------------------------------
test_name="GET /node/info returns 200 with pubkey"
response=$(curl -sf "${PRIMARY_URL}/node/info" 2>/dev/null || echo "CURL_FAILED")
status=$(curl -s -o /dev/null -w '%{http_code}' "${PRIMARY_URL}/node/info")
if [ "$status" = "200" ] && echo "$response" | grep -q "pubkey"; then
    pass "$test_name"
else
    fail "$test_name (status=$status)"
fi

# --------------------------------------------------------------------------
# Test 4: POST /v1/proofs/challenge with valid data returns 201
# --------------------------------------------------------------------------
test_name="POST /v1/proofs/challenge with valid data returns 201"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${PRIMARY_URL}/v1/proofs/challenge" \
    -H "Content-Type: application/json" \
    -d '{"feed_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","scope":"feed:write","requester_nonce":"abcdefghijklmnopqrstuvwxyz"}')
if [ "$status" = "201" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 5: POST /v1/proofs/challenge with short nonce returns 400 (not 500)
# --------------------------------------------------------------------------
test_name="POST /v1/proofs/challenge with short nonce returns 400"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${PRIMARY_URL}/v1/proofs/challenge" \
    -H "Content-Type: application/json" \
    -d '{"feed_guid":"test-guid","scope":"feed:write","requester_nonce":"short"}')
if [ "$status" = "400" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 6: POST /v1/proofs/challenge with bad scope returns 400
# --------------------------------------------------------------------------
test_name="POST /v1/proofs/challenge with bad scope returns 400"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${PRIMARY_URL}/v1/proofs/challenge" \
    -H "Content-Type: application/json" \
    -d '{"feed_guid":"test-guid","scope":"admin:nuke","requester_nonce":"abcdefghijklmnopqrstuvwxyz"}')
if [ "$status" = "400" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 7: DELETE /v1/feeds/nonexistent WITH admin token returns 404 (not 500)
# The feed does not exist, so the handler should return 404 after auth passes.
# --------------------------------------------------------------------------
test_name="DELETE /v1/feeds/nonexistent with admin token returns 404"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X DELETE "${PRIMARY_URL}/v1/feeds/nonexistent-guid" \
    -H "X-Admin-Token: ${ADMIN_TOKEN}")
if [ "$status" = "404" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 8: DELETE /v1/feeds/nonexistent WITHOUT any auth returns 401
#         with WWW-Authenticate header (RFC 6750)
# --------------------------------------------------------------------------
test_name="DELETE /v1/feeds/nonexistent without token returns 401 + WWW-Authenticate"
response_headers=$(curl -s -D - -o /dev/null \
    -X DELETE "${PRIMARY_URL}/v1/feeds/nonexistent-guid")
status=$(echo "$response_headers" | head -1 | grep -o '[0-9]\{3\}')
has_www_auth=$(echo "$response_headers" | grep -ci 'WWW-Authenticate' || true)
if [ "$status" = "401" ] && [ "$has_www_auth" -ge 1 ]; then
    pass "$test_name"
else
    fail "$test_name (status=$status, WWW-Authenticate present=$has_www_auth)"
fi

# --------------------------------------------------------------------------
# Test 9: DELETE /v1/feeds/nonexistent with WRONG admin token returns 403
# --------------------------------------------------------------------------
test_name="DELETE /v1/feeds/nonexistent with wrong admin token returns 403"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X DELETE "${PRIMARY_URL}/v1/feeds/nonexistent-guid" \
    -H "X-Admin-Token: wrong-token")
if [ "$status" = "403" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 10: POST /v1/proofs/challenge with missing fields returns 422 (not 500)
# Axum returns 422 for JSON deserialization failures.
# --------------------------------------------------------------------------
test_name="POST /v1/proofs/challenge with missing fields returns 422"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "${PRIMARY_URL}/v1/proofs/challenge" \
    -H "Content-Type: application/json" \
    -d '{"feed_guid":"test"}')
if [ "$status" = "422" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 11: Community node health check (community-1 on port 8009)
# --------------------------------------------------------------------------
test_name="Community node community-1 returns 200 on /health"
status=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:8009/health" 2>/dev/null || echo "000")
if [ "$status" = "200" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# --------------------------------------------------------------------------
# Test 12: Community node is read-only (POST /ingest/feed should fail)
# Community nodes do not expose /ingest/feed — expect 404 or 405.
# --------------------------------------------------------------------------
test_name="Community node rejects POST /ingest/feed"
status=$(curl -s -o /dev/null -w '%{http_code}' \
    -X POST "http://localhost:8009/ingest/feed" \
    -H "Content-Type: application/json" \
    -d '{}' 2>/dev/null || echo "000")
if [ "$status" = "404" ] || [ "$status" = "405" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status, expected 404 or 405)"
fi

# --------------------------------------------------------------------------
# Test 13: MITM proxy is forwarding to primary (GET /health via port 8080)
# --------------------------------------------------------------------------
test_name="MITM proxy forwards /health to primary"
status=$(curl -s -o /dev/null -w '%{http_code}' "http://localhost:8080/health" 2>/dev/null || echo "000")
if [ "$status" = "200" ]; then
    pass "$test_name"
else
    fail "$test_name (got $status)"
fi

# ── Summary ─────────────────────────────────────────────────────────────────

echo ""
log "=========================================="
log "  E2E Security Smoke Test Results"
log "=========================================="
for t in "${TESTS[@]}"; do
    if echo "$t" | grep -q "^PASS"; then
        printf '  \033[1;32m%s\033[0m\n' "$t"
    else
        printf '  \033[1;31m%s\033[0m\n' "$t"
    fi
done
echo ""
log "Total: $((PASSED + FAILED))  Passed: ${PASSED}  Failed: ${FAILED}"
log "=========================================="

if [ "$FAILED" -gt 0 ]; then
    log "Some tests failed. Dumping primary logs for debugging:"
    docker compose -f "$COMPOSE_FILE" logs primary 2>/dev/null || true
    exit 1
fi

log "All tests passed."
exit 0
