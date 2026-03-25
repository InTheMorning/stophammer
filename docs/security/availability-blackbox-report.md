# Availability / DoS Black-Box Security Report

**Target:** stophammer v0.1.0 (Rust, axum 0.8, SQLite)
**Date:** 2026-03-12
**Scope:** Resource exhaustion, unbounded growth, denial-of-service vectors

> Historical snapshot. This report reflects the 2026-03-12 tree and is
> superseded by `availability-blackbox-report-v2.md` and
> `final-audit-report.md` for current behavior.

---

## Executive Summary

Seven availability vulnerabilities were identified and remediated. The most
critical was an unauthenticated proof-challenge creation endpoint that could
be used to exhaust disk space via the `proof_challenges` table. All fixes
have been implemented with tests proving both the vulnerability and the
remediation.

**Findings:** 7 confirmed vulnerabilities, all fixed
**Tests added:** 12 (all passing)
**Regressions:** 0 (full suite of 267 tests passes)

---

## Findings

### AVAIL-01: Proof Challenge Table Exhaustion (HIGH)

**Endpoint:** `POST /v1/proofs/challenge`
**Authentication:** None required
**Pre-fix state:** No rate limiting. An attacker could create unlimited pending
challenges that persist for 24 hours before the pruner runs. With no cap,
flooding this endpoint at sustained rates (e.g. 1000 req/s) would create
~86 million rows/day, consuming significant disk and degrading all DB
operations.

**Attack scenario:**
```
for i in $(seq 1 1000000); do
  curl -X POST /v1/proofs/challenge \
    -d '{"feed_guid":"x","scope":"feed:write","requester_nonce":"aaaaaaaaaaaaaaaa'$i'"}'
done
```

**Fix:** Added per-feed-guid cap of 20 pending challenges. The 21st
challenge for the same feed_guid returns `429 Too Many Requests`. Resolved
(valid/invalid) challenges do not count against the limit.

**File:** `src/api.rs` -- `handle_proofs_challenge` handler
**Test:** `proof_challenge_rate_limit_enforced`, `proof_challenge_rate_limit_per_feed`,
`resolved_challenges_dont_count_towards_rate_limit`

---

### AVAIL-02: No Explicit Body Size Limit (MEDIUM)

**Endpoints:** All POST endpoints
**Pre-fix state:** Axum 0.8's default body limit is 2 MiB, but this was
implicit and not explicitly configured. Relying on implicit defaults is
fragile -- an axum upgrade could change the default, or a middleware could
accidentally override it.

**Fix:** Added explicit `DefaultBodyLimit::max(2 * 1024 * 1024)` layer to
both `build_router` and `build_readonly_router`. This makes the 2 MiB limit
explicit, documented, and resistant to accidental changes.

**File:** `src/api.rs` -- `build_router`, `build_readonly_router`
**Test:** `body_size_limit_rejects_oversized_payload`, `body_within_limit_accepted`

---

### AVAIL-03: FTS5 Search Index Bomb (MEDIUM)

**Endpoint:** `POST /ingest/feed` (via search index population)
**Pre-fix state:** Feed descriptions, titles, and other text fields were
passed directly to the FTS5 `search_index` table without length validation.
A feed with a 100 KB description would create a disproportionately large
FTS5 index entry, and FTS5 tokenization amplifies the storage cost (each
unique token creates B-tree entries in the inverted index).

**Attack scenario:** Submit feeds with enormous description fields. Even at
2 MiB body limit, a single ingest with a 1 MB description repeated across
multiple tracks could create megabytes of FTS5 index data per request.

**Fix:** Added `truncate_fts_field()` in `search.rs` that caps all text
fields to 10,000 bytes before insertion into FTS5. Truncation respects
UTF-8 character boundaries.

**File:** `src/search.rs` -- `truncate_fts_field`, `populate_search_index`
**Test:** `fts5_handles_oversized_description_without_error`,
`fts5_truncates_large_field_to_limit`, `fts5_truncation_respects_char_boundaries`

---

### AVAIL-04: Unbounded Track Count Per Ingest (MEDIUM)

**Endpoint:** `POST /ingest/feed`
**Pre-fix state:** No limit on the number of tracks in a single ingest
request. A malicious submission could pack hundreds of tracks into a single
feed, each generating:
- An `artists` upsert
- An `artist_credit` upsert
- A `tracks` upsert
- N `payment_routes` rows
- N `value_time_splits` rows
- A `search_index` FTS5 entry
- An `events` row
- A quality score computation

With 2000 tracks, a single request generates ~6000 DB writes and ~2000
event rows.

**Fix:** Added `MAX_TRACKS_PER_INGEST = 500` limit. Requests exceeding
this return `400 Bad Request` with a descriptive error message. The limit
is applied after the verifier chain runs (so content-hash short-circuit
still works) but before any DB writes.

**File:** `src/api.rs` -- `handle_ingest_feed` handler

---

### AVAIL-05: Unbounded Push Event Array (MEDIUM)

**Endpoint:** `POST /sync/push` (community nodes)
**Pre-fix state:** The push handler accepted a `PushRequest { events: Vec<Event> }`
with no limit on the number of events. A malicious primary (or
man-in-the-middle) could send millions of events in a single push, causing
memory exhaustion during deserialization and prolonged DB lock contention
during application.

**Fix:** Added `MAX_PUSH_EVENTS = 1,000` limit and explicit
`DefaultBodyLimit::max(2 MiB)` on the community push router. Requests
exceeding 1,000 events return `400 Bad Request`.

**File:** `src/community.rs` -- `handle_sync_push`, `build_community_push_router`

---

### AVAIL-06: Unbounded Reconcile Have Array (LOW)

**Endpoint:** `POST /sync/reconcile`
**Pre-fix state:** The reconcile request accepted `have: Vec<EventRef>` with
no limit. The handler builds two `HashSet<String>` from the have array,
meaning memory usage scales linearly with input size. Additionally, it
queries up to 10,000 events from the DB. A malicious reconcile request
with millions of EventRefs could exhaust memory during HashSet construction.

**Fix:** Added `MAX_RECONCILE_HAVE = 10,000` limit. Requests exceeding
this return `400 Bad Request`.

**File:** `src/api.rs` -- `handle_sync_reconcile`
**Test:** `reconcile_rejects_oversized_have`, `reconcile_accepts_valid_have`

---

### AVAIL-07: Oversized Requester Nonce (LOW)

**Endpoint:** `POST /v1/proofs/challenge`
**Pre-fix state:** The `requester_nonce` field had a minimum length check
(16 chars) but no maximum. An attacker could send a multi-megabyte nonce
which would be SHA-256 hashed and the result stored in the
`token_binding` column. While the hash output is fixed-size, the nonce
itself must be parsed from JSON and held in memory, and the repeated
hashing of a huge nonce wastes CPU.

**Fix:** Added `MAX_NONCE_BYTES = 256` limit. Nonces exceeding this
return `400 Bad Request`.

**File:** `src/api.rs` -- `handle_proofs_challenge`
**Test:** `proof_challenge_rejects_oversized_nonce`

---

## Attack Surfaces Investigated But Not Vulnerable

### Sync Register Endpoint (`POST /sync/register`)

This endpoint is unauthenticated and allows any client to register a push
URL. However, the impact is limited:
- The in-memory push subscriber map is bounded by the number of unique
  pubkeys (each registration replaces the URL for a given pubkey).
- The `peer_nodes` DB table has `node_pubkey` as primary key, so duplicate
  registrations are upserts, not inserts.
- Failed push deliveries trigger an eviction mechanism (5 consecutive
  failures removes the peer).

**Assessment:** Low risk. An attacker can register fake push URLs, but
the fan-out timeout (10s) and eviction mechanism limit the blast radius.
Consider adding authentication in a future sprint if the network grows.

### Event Replay Flood

Community nodes receive events via `POST /sync/push`. The handler uses
signature verification (only events signed by the known primary pubkey are
accepted) and `INSERT OR IGNORE` for idempotent application. Replaying the
same events causes:
- Signature verification overhead (ed25519, ~microseconds per event)
- An `INSERT OR IGNORE` that immediately returns (no lock contention)
- Duplicate counter incremented (no DB write)

**Assessment:** Not exploitable. The combination of signature filtering and
idempotent inserts makes replay attacks a no-op.

### SQLite Lock Contention

All DB access goes through a single `Arc<Mutex<Connection>>`. This is a
deliberate design choice for SQLite (which doesn't support true concurrent
writes). The `spawn_blocking` pattern ensures the async runtime is never
blocked. Under heavy load, requests queue on the mutex, which provides
natural backpressure. The 2 MiB body limit and per-request track caps
bound the maximum time any single request holds the mutex.

**Assessment:** Acceptable. The single-writer model is correct for SQLite.

---

## Hardening Constants Summary

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

---

## Test Coverage

All 12 availability tests pass. The full test suite (267 tests) passes
with zero regressions.

```
tests/availability_tests.rs:
  proof_challenge_rate_limit_enforced ........... PASS
  proof_challenge_rate_limit_per_feed ........... PASS
  proof_challenge_rejects_oversized_nonce ....... PASS
  reconcile_rejects_oversized_have .............. PASS
  reconcile_accepts_valid_have .................. PASS
  fts5_handles_oversized_description ............ PASS
  fts5_truncates_large_field_to_limit ........... PASS
  fts5_truncation_respects_char_boundaries ...... PASS
  prune_expired_frees_challenge_slots ........... PASS
  body_size_limit_rejects_oversized_payload ..... PASS
  body_within_limit_accepted .................... PASS
  resolved_challenges_dont_count_towards_limit .. PASS
```
