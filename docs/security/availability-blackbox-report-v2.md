# Availability / DoS Black-Box Security Report (v2)

**Target:** stophammer v0.1.0 (Rust, axum 0.8, SQLite)
**Date:** 2026-03-13
**Scope:** Re-audit of v1 availability findings + new attack surfaces from
FG-02 (SSE), CS-01 (RSS fetch), SP-03 (rate limiting), SP-04 (push retry),
FG-01 (structured logging).

---

## Executive Summary

All seven v1 findings remain closed. Three new availability vulnerabilities
were identified and remediated:

- **AVAIL-08 (HIGH):** RSS response body unbounded -- OOM via streaming
  response from attacker-controlled feed URL.
- **AVAIL-09 (MEDIUM):** SSE registry unbounded artist entries -- memory
  exhaustion via fabricated artist IDs.
- **AVAIL-10 (MEDIUM):** No concurrent SSE connection limit -- resource
  exhaustion via persistent connection flooding.

**Findings:** 3 new vulnerabilities, all fixed
**Tests added:** 5 (17 total availability tests, all passing)
**Regressions:** 0 (full suite of 330 tests passes)

---

## Part 1: Re-verification of v1 Findings

### AVAIL-01: Proof Challenge Table Exhaustion -- CLOSED

The per-feed-guid cap of 20 pending challenges remains in place at
`api.rs:1948-1961`. The `MAX_PENDING_CHALLENGES_PER_FEED = 20` constant and
the `SELECT COUNT(*)` guard are unchanged.

**Status:** CLOSED (no regression)

### AVAIL-02: No Body Size Limit -- CLOSED

`DefaultBodyLimit::max(2 * 1024 * 1024)` is applied to both `build_router`
(line 459) and `build_readonly_router` (line 474), and separately on the
community push router via `MAX_PUSH_BODY_BYTES` in `community.rs:34`.

**Status:** CLOSED (no regression)

### AVAIL-03: FTS5 Field Size -- CLOSED

`truncate_fts_field()` in `search.rs:23-34` truncates all text fields to
`MAX_FTS_FIELD_BYTES = 10_000` bytes before FTS5 insertion.

**Status:** CLOSED (no regression)

### AVAIL-04: Unbounded Track Creation -- CLOSED

`MAX_TRACKS_PER_INGEST = 500` enforced at `api.rs:536`.

**Status:** CLOSED (no regression)

### AVAIL-05: Push Event Flood -- CLOSED

`MAX_PUSH_EVENTS = 1_000` enforced at `community.rs:31,330`.

**Status:** CLOSED (no regression)

### AVAIL-06: Reconcile Array -- CLOSED

`MAX_RECONCILE_HAVE = 10_000` enforced at `api.rs:1026`.

**Status:** CLOSED (no regression)

### AVAIL-07: Nonce Size -- CLOSED

`MAX_NONCE_BYTES = 256` enforced at `api.rs:1929`.

**Status:** CLOSED (no regression)

---

## Part 2: New Attack Surface Analysis

### AVAIL-08: RSS Response Body Unbounded (HIGH) -- FIXED

**Endpoint:** `POST /v1/proofs/assert` (triggers RSS fetch via
`verify_podcast_txt`)
**Pre-fix state:** The `verify_podcast_txt` function in `proof.rs` fetched
the RSS response using `resp.text().await` with no size limit. While the
request has a 10-second timeout, a fast server can stream 100+ MB in that
window. An attacker controlling a feed_url could:

1. Create a challenge for a feed whose `feed_url` points to their server.
2. Assert the challenge, triggering an outbound RSS fetch.
3. Their server streams an infinite response body.
4. The stophammer server allocates unbounded memory reading the body.
5. With 50 RPS rate limit, 50 concurrent in-flight fetches could consume
   gigabytes of memory.

**Attack scenario:**
```
# Attacker's tar pit server streams infinite data:
python3 -c "
import http.server, socketserver
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        while True:
            self.wfile.write(b'x' * 1048576)
" &

# Attacker repeatedly triggers RSS fetch:
for i in $(seq 1 50); do
  curl -X POST /v1/proofs/assert \
    -d '{"challenge_id":"$CID","requester_nonce":"$NONCE"}' &
done
```

**Fix:** Added `MAX_RSS_BODY_BYTES = 5 * 1024 * 1024` (5 MiB) in `proof.rs`.
The function now:
1. Checks `Content-Length` header before reading (fast reject).
2. Uses `resp.bytes().await` and checks the actual byte count.
3. Returns an error if either check fails.

5 MiB is generous for any legitimate RSS feed. The largest known podcast
feeds are under 2 MiB.

**File:** `src/proof.rs` -- `verify_podcast_txt`, `MAX_RSS_BODY_BYTES`
**Test:** `rss_body_size_limit_rejects_oversized_response`,
`rss_body_within_limit_accepted`

---

### AVAIL-09: SSE Registry Unbounded Artist Entries (MEDIUM) -- FIXED

**Endpoint:** `GET /v1/events?artists=id1,...,id50000`
**Pre-fix state:** The `SseRegistry` created a new broadcast channel and
ring buffer for every unique artist ID requested via SSE subscriptions.
While each connection was limited to 50 artists (`MAX_SSE_ARTISTS`), there
was no limit on the total number of unique artists across all connections.
An attacker could:

1. Make 1000 SSE connections, each with 50 unique fabricated artist IDs.
2. This creates 50,000 broadcast channels + 50,000 ring buffers.
3. Each broadcast channel allocates a 256-slot buffer.
4. Memory grows linearly with the number of unique artist IDs.

**Fix:** Added `MAX_SSE_REGISTRY_ARTISTS = 10_000` limit in `api.rs`.
The `SseRegistry::subscribe()` method now returns `Option<Receiver>` and
refuses to create new entries when the registry is full. The
`SseRegistry::publish()` method also respects this limit for the ring
buffer map. Legitimate artists already in the registry are unaffected.

**File:** `src/api.rs` -- `SseRegistry::subscribe`, `SseRegistry::publish`,
`MAX_SSE_REGISTRY_ARTISTS`
**Test:** `sse_registry_limits_artist_entries`

---

### AVAIL-10: No Concurrent SSE Connection Limit (MEDIUM) -- FIXED

**Endpoint:** `GET /v1/events`
**Pre-fix state:** Each SSE connection holds a long-lived tokio task that
polls broadcast receivers at 100ms intervals. With no connection limit, an
attacker could open thousands of persistent SSE connections, each consuming:
- A tokio task (stack memory + scheduler overhead)
- Broadcast receiver memory for each subscribed artist
- 100ms timer wakeups contributing to scheduler contention

The per-IP rate limiter (50 RPS / 100 burst) provides only weak protection
because:
- SSE connections are established with a single request.
- After establishment, the connection persists indefinitely.
- An attacker with a few IPs can create thousands of connections.

**Fix:** Added `MAX_SSE_CONNECTIONS = 1_000` server-wide limit enforced by
an atomic counter in `SseRegistry`. The handler calls
`try_acquire_connection()` before subscribing; if the limit is reached, it
returns `503 Service Unavailable`. An RAII guard (`SseConnectionGuard`)
releases the slot when the stream is dropped (client disconnects).

1,000 concurrent SSE connections is generous for any deployment that also
has rate limiting. For comparison, a typical Axum deployment handles
~10,000 concurrent TCP connections before other limits (fd limits, memory)
become the bottleneck.

**File:** `src/api.rs` -- `handle_sse_events`, `SseConnectionGuard`,
`MAX_SSE_CONNECTIONS`, `SseRegistry::try_acquire_connection`,
`SseRegistry::release_connection`
**Test:** `sse_connection_limit_enforced`,
`sse_endpoint_rejects_when_connections_full`

---

## Part 3: Attack Surfaces Investigated But Not Vulnerable

### RSS Fetch as Tokio Thread Pool Exhaustion

**Hypothesis:** With 50 RPS rate limit and 10-second RSS timeout, an attacker
could have 500 concurrent outbound HTTP requests in-flight, exhausting the
tokio worker thread pool.

**Assessment:** PROTECTED. The `verify_podcast_txt` function is fully async
(uses `reqwest::Client` which is built on `hyper` + `tokio::net`). The
outbound HTTP request does not block a worker thread; it yields to the
runtime. The 10-second timeout (`Duration::from_secs(10)`) on each request
ensures that even slow-trickle responses are aborted. Combined with the new
5 MiB body cap (AVAIL-08), the resource consumption per in-flight request
is bounded:

- Memory: max 5 MiB per request * 500 concurrent = 2.5 GB worst case.
  In practice, the rate limiter prevents sustained 50 RPS to the assert
  endpoint specifically (requires a valid pending challenge each time).
- CPU: negligible (waiting on I/O).

The SSRF guard (`validate_feed_url`) additionally blocks requests to
private/reserved IPs, preventing the server from being used as a port
scanner.

### Rate Limiter Bypass via X-Forwarded-For

**Hypothesis:** An attacker can spoof `X-Forwarded-For` to bypass per-IP
rate limiting.

**Assessment:** PROTECTED. The rate limiter in `main.rs:234-256` reads
`X-Forwarded-For` only when `TRUST_PROXY=true` (environment variable).
The default is `false`, which uses `ConnectInfo<SocketAddr>` from the
kernel TCP connection. This cannot be spoofed.

When `TRUST_PROXY=true` is set (behind a reverse proxy), the first hop in
`X-Forwarded-For` is extracted. This is secure when the reverse proxy
(e.g., Cloudflare, nginx) strips/overwrites `X-Forwarded-For` on ingress.

**Code reference:** `main.rs:248-256`

### Push Retry Resource Consumption

**Hypothesis:** An attacker registers 100 fake push peers, and each failed
push now occupies a tokio task for 1.5 seconds (500ms + 1000ms retries)
instead of failing immediately.

**Assessment:** LOW RISK. Push peer registration (`POST /sync/register`)
now requires `X-Admin-Token` authentication (`api.rs:1237`). An attacker
cannot register fake peers without the admin token. Even if the admin token
is compromised, the blast radius is bounded:

- `PUSH_MAX_ATTEMPTS = 3` with delays of 0ms, 500ms, 1000ms.
- Each push task completes in at most 31.5 seconds (3 attempts * 10s
  timeout + 0.5s + 1.0s delays).
- Eviction after `PUSH_EVICTION_THRESHOLD = 10` consecutive failures
  permanently removes the peer from the in-memory cache.
- The number of peers is bounded by authenticated registrations.

### Broadcast Channel Backpressure

**Hypothesis:** The `broadcast::Sender` with capacity 256 could block the
ingest path if a slow consumer doesn't read.

**Assessment:** PROTECTED. `tokio::sync::broadcast::Sender::send()` is
non-blocking. When the buffer is full, the oldest message is dropped and
slow receivers get a `Lagged` error on their next `recv()`. The `publish()`
method in `SseRegistry` uses `let _ = tx.send(frame)` which discards the
error (correct behavior: publisher should never block).

The SSE live stream handles `TryRecvError::Lagged(n)` by logging at debug
level and continuing. This is the correct behavior for an SSE endpoint
where occasional message loss is acceptable.

### Tracing Log Flood

**Hypothesis:** An attacker triggers excessive structured logging (e.g.,
many failed auth attempts) that fills disk or consumes CPU.

**Assessment:** LOW RISK. The tracing configuration in `main.rs:13-17` uses
`EnvFilter` defaulting to `stophammer=info`. At `info` level:
- Failed auth attempts do not generate logs (the error is returned to the
  client, not logged).
- Rate-limited requests generate no log output.
- Only significant operational events (push processing, peer eviction,
  sync operations) generate logs.

The per-IP rate limiter (50 RPS) caps the total request volume from any
single source. Even if all 50 RPS generate log lines, this is ~4.3M lines
per day, well within typical log rotation capabilities.

The `debug` and `trace` levels would generate more output but are not
enabled by default.

---

## Hardening Constants Summary (Updated)

| Constant | Value | File |
|---|---|---|
| `MAX_BODY_BYTES` | 2 MiB | `api.rs` |
| `MAX_TRACKS_PER_INGEST` | 500 | `api.rs` |
| `MAX_RECONCILE_HAVE` | 10,000 | `api.rs` |
| `MAX_PENDING_CHALLENGES_PER_FEED` | 20 | `api.rs` |
| `MAX_NONCE_BYTES` | 256 | `api.rs` |
| `MAX_FTS_FIELD_BYTES` | 10,000 | `search.rs` |
| `MAX_PUSH_EVENTS` | 1,000 | `community.rs` |
| `MAX_PUSH_BODY_BYTES` | 2 MiB | `community.rs` |
| `MAX_RSS_BODY_BYTES` | 5 MiB | `proof.rs` (NEW) |
| `MAX_SSE_REGISTRY_ARTISTS` | 10,000 | `api.rs` (NEW) |
| `MAX_SSE_CONNECTIONS` | 1,000 | `api.rs` (NEW) |
| `SSE_CHANNEL_CAPACITY` | 256 | `api.rs` |
| `SSE_RING_BUFFER_SIZE` | 100 | `api.rs` |
| `MAX_SSE_ARTISTS` (per conn) | 50 | `api.rs` |

---

## Test Coverage

All 17 availability tests pass. The full test suite (330 tests) passes
with zero regressions.

```
tests/availability_tests.rs:
  proof_challenge_rate_limit_enforced ............. PASS  (v1)
  proof_challenge_rate_limit_per_feed ............. PASS  (v1)
  proof_challenge_rejects_oversized_nonce ......... PASS  (v1)
  reconcile_rejects_oversized_have ................ PASS  (v1)
  reconcile_accepts_valid_have .................... PASS  (v1)
  fts5_handles_oversized_description .............. PASS  (v1)
  fts5_truncates_large_field_to_limit ............. PASS  (v1)
  fts5_truncation_respects_char_boundaries ........ PASS  (v1)
  prune_expired_frees_challenge_slots ............. PASS  (v1)
  body_size_limit_rejects_oversized_payload ....... PASS  (v1)
  body_within_limit_accepted ...................... PASS  (v1)
  resolved_challenges_dont_count_towards_limit .... PASS  (v1)
  rss_body_size_limit_rejects_oversized_response .. PASS  (v2 NEW)
  rss_body_within_limit_accepted .................. PASS  (v2 NEW)
  sse_registry_limits_artist_entries .............. PASS  (v2 NEW)
  sse_connection_limit_enforced ................... PASS  (v2 NEW)
  sse_endpoint_rejects_when_connections_full ....... PASS  (v2 NEW)
```
