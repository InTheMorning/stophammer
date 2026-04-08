#!/usr/bin/env bash
set -euo pipefail

# Resolver-aware load test for stophammer.
#
# This script intentionally does not treat search freshness as an ingest-path
# property anymore. It distinguishes:
#   - immediate source-layer reads
#   - resolver-backed canonical/search reads
#
# Prerequisites:
#   - `hey` installed: go install github.com/rakyll/hey@latest
#   - stophammer running at PRIMARY_URL (default: http://localhost:8008)
#   - one seeded source feed GUID for source reads
#   - optional seeded canonical/search query for resolver-backed reads
#
# Usage:
#   FEED_GUID=<guid> ./tests/load_test.sh
#   FEED_GUID=<guid> TRACK_GUID=<guid> ./tests/load_test.sh
#   FEED_GUID=<guid> SEARCH_QUERY=artist WAIT_FOR_RESOLVER=1 ./tests/load_test.sh

PRIMARY_URL="${PRIMARY_URL:-http://localhost:8008}"
FEED_GUID="${FEED_GUID:-}"
TRACK_GUID="${TRACK_GUID:-}"
SEARCH_QUERY="${SEARCH_QUERY:-}"
CONCURRENCY="${CONCURRENCY:-20}"
TOTAL_REQUESTS="${TOTAL_REQUESTS:-100}"
WAIT_FOR_RESOLVER="${WAIT_FOR_RESOLVER:-0}"
RESOLVER_WAIT_TIMEOUT_SECS="${RESOLVER_WAIT_TIMEOUT_SECS:-120}"
SOURCE_P99_THRESHOLD_MS="${SOURCE_P99_THRESHOLD_MS:-1500}"
CANONICAL_P99_THRESHOLD_MS="${CANONICAL_P99_THRESHOLD_MS:-2500}"

PASSED=0
FAILED=0
TESTS=()

log()  { printf '\033[1;34m[load]\033[0m %s\n' "$*"; }
pass() { PASSED=$((PASSED + 1)); TESTS+=("PASS: $1"); printf '\033[1;32m  PASS\033[0m %s\n' "$1"; }
fail() { FAILED=$((FAILED + 1)); TESTS+=("FAIL: $1"); printf '\033[1;31m  FAIL\033[0m %s\n' "$1"; }

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        log "ERROR: '$1' is required but not installed."
        exit 1
    fi
}

parse_json_bool() {
    sed -n 's/.*"caught_up":[[:space:]]*\(true\|false\).*/\1/p'
}

parse_hey_metric() {
    local label="$1"
    local output="$2"
    echo "$output" | grep "$label" | awk '{print $3}' | sed 's/secs//'
}

check_target() {
    log "Checking target is reachable at ${PRIMARY_URL}/health..."
    if ! curl -sf "${PRIMARY_URL}/health" >/dev/null 2>&1; then
        log "ERROR: ${PRIMARY_URL}/health is not reachable."
        exit 1
    fi
}

run_get_load() {
    local name="$1"
    local url="$2"
    local threshold_ms="$3"

    log "Running ${name}: ${TOTAL_REQUESTS} requests, concurrency ${CONCURRENCY}..."
    local output
    output="$(hey -n "$TOTAL_REQUESTS" -c "$CONCURRENCY" "$url" 2>&1)"

    local p50 p95 p99 rps
    p50="$(parse_hey_metric '50% in' "$output")"
    p95="$(parse_hey_metric '95% in' "$output")"
    p99="$(parse_hey_metric '99% in' "$output")"
    rps="$(echo "$output" | grep 'Requests/sec:' | awk '{print $2}')"

    log "  ${name} p50: ${p50:-N/A}s  p95: ${p95:-N/A}s  p99: ${p99:-N/A}s"
    log "  ${name} throughput: ${rps:-N/A} req/s"

    if [ -z "${p99}" ]; then
        fail "${name} (could not parse p99 latency)"
        return
    fi

    local p99_ms
    p99_ms="$(echo "$p99" | awk "{printf \"%.0f\", \$1 * 1000}")"
    if [ "$p99_ms" -le "$threshold_ms" ]; then
        pass "${name} (p99=${p99_ms}ms, threshold=${threshold_ms}ms)"
    else
        fail "${name} (p99=${p99_ms}ms exceeds threshold=${threshold_ms}ms)"
    fi
}

require_cmd hey
check_target

if [ -z "$FEED_GUID" ]; then
    log "ERROR: FEED_GUID is required for source-layer load testing."
    exit 1
fi

run_get_load \
    "Health endpoint baseline" \
    "${PRIMARY_URL}/health" \
    500

run_get_load \
    "Source feed detail" \
    "${PRIMARY_URL}/v1/feeds/${FEED_GUID}" \
    "$SOURCE_P99_THRESHOLD_MS"

if [ -n "$TRACK_GUID" ]; then
    run_get_load \
        "Source track detail" \
        "${PRIMARY_URL}/v1/tracks/${TRACK_GUID}" \
        "$SOURCE_P99_THRESHOLD_MS"
fi

if [ -n "$SEARCH_QUERY" ]; then
    run_get_load \
        "Search" \
        "${PRIMARY_URL}/v1/search?q=${SEARCH_QUERY}" \
        "$CANONICAL_P99_THRESHOLD_MS"
fi

echo ""
log "=========================================="
log "  Resolver-Aware Load Test Results"
log "=========================================="
echo ""
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
    exit 1
fi

log "All load checks passed."
