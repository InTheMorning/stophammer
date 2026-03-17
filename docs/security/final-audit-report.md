## Final Audit Report

**Date:** 2026-03-12
**Target:** stophammer v0.1.0

---

### Build Status

- **cargo build --release:** PASS (zero errors, zero warnings)
- **cargo test:** PASS (247 passed, 0 failed, 1 ignored)
- **cargo clippy --all-targets -- -D warnings -D clippy::pedantic -D clippy::nursery:** PASS (zero warnings)

---

### Sprint Completion Summary

- **Sprint 1A: Auth atomicity TOCTOU fix (Issue #1)** -- Complete. All authenticated handlers acquire the DB mutex before performing auth checks, holding the lock through both authentication and the subsequent database mutation. `check_admin_or_bearer_with_conn` operates on an already-locked `&Connection`, eliminating TOCTOU between auth check and DB write. 11 tests in `auth_atomicity_tests.rs` verify correctness.

- **Sprint 1B: RFC 6750 compliance (Issues #4, #6, #8, #11)** -- Complete. `WWW-Authenticate` header emitted on all auth failures: `Bearer realm="stophammer"` for missing credentials, `Bearer realm="stophammer", error="invalid_token"` for bad tokens, `Bearer realm="stophammer", error="insufficient_scope"` for wrong-feed tokens. `extract_bearer_token` handles whitespace trimming, empty tokens, and non-Bearer schemes. 12 tests in `rfc6750_tests.rs` verify correctness.

- **Sprint 2A: Fan-out hard error + /v1/ prefix + RSS skip test (Issues #2, #3, #7)** -- Complete. All REST endpoints mount under `/v1/` prefix. `ContentHashVerifier` returns `NO_CHANGE` when the cached hash matches, allowing RSS skip optimization. 9 tests in `sprint2a_tests.rs` verify correctness including v1 prefix routing for feeds, tracks, proofs/challenge, and proofs/assert.

- **Sprint 2B: Mutex poison propagation + spawn_db helper (Issues #5, #14)** -- Complete. `PoisonedDb` error variant added. `spawn_db` and `spawn_db_mut` helpers encapsulate `spawn_blocking` + mutex acquisition with proper poison propagation. All DB access paths surface mutex poison errors as HTTP 500. 8 tests in `sprint2b_tests.rs` verify correctness.

- **Sprint 3A: proof.rs recompute_binding + expiry pruning (Issues #9, #17)** -- Complete. `recompute_binding` performs `split_once('.')` with empty-part guards. `prune_expired` deletes expired challenges, freeing rate-limit slots. 22 tests in `proof_tests.rs` cover full challenge-assert-token lifecycle including binding recomputation, expiry pruning, and round-trip integration.

- **Sprint 3B: TLS Send+Sync + key path rename + GeneralizedTime test (Issues #10, #18, #19)** -- Complete. `provision_certificate` future is `Send` (verified by compile-time assertion). TLS key path reads from `STOPHAMMER_TLS_KEY_PATH` environment variable matching ADR-0019. `cert_needs_renewal` handles `GeneralizedTime` format (YYYYMMDDHHmmSSZ). 8 tests in `tls_tests.rs` verify correctness.

- **Sprint 3C: N*DELETE subquery optimization (Issue #12)** -- Complete. Feed and track deletion uses `DELETE FROM ... WHERE feed_guid = ?` with single-pass subquery patterns rather than N individual DELETE statements. `delete_feed_with_event` wraps all deletions in a single transaction. 10 tests in `retire_remove_tests.rs` verify correctness including many-tracks cascade scenarios.

- **Sprint 4A: PATCH REST semantics 204 No Content (Issue #13)** -- Complete. `PATCH /v1/feeds/{guid}` and `PATCH /v1/tracks/{guid}` return `204 No Content` with empty body on success, including the empty-body/no-fields case. 5 tests in `sprint4a_tests.rs` verify correctness.

- **Sprint 4B: Full round-trip test + timestamp ordering (Issues #15, #16)** -- Complete. `full_roundtrip_challenge_assert_patch_feed_and_track` in `proof_tests.rs` exercises the complete lifecycle: challenge creation, nonce assertion, token issuance, feed PATCH, and track PATCH. `value_time_splits_ordering` in `regression_tests.rs` and `tracks_ordered_by_pub_date` in `track_tests.rs` verify timestamp ordering.

---

### Security Blackbox Findings

**Auth researcher (tests/security_auth_tests.rs -- 17 tests):**
- 7 attack vectors tested with concrete proof-of-concept code
- Token replay after feed deletion: PROTECTED (subject GUID mismatch prevents cross-feed use; orphaned token PATCH is a harmless no-op)
- Challenge race condition (double-spend): PROTECTED (SQLite mutex + SQL-level `WHERE state = 'pending'` idempotency)
- Cross-feed token: PROTECTED (`check_admin_or_bearer_with_conn` compares subject GUID against path parameter)
- Admin token timing attack: PARTIALLY_PROTECTED (non-constant-time `==` comparison; practically infeasible over network due to jitter vs. sub-nanosecond signal)
- Bearer token format bypass (empty, whitespace, long, special chars, non-Bearer scheme): PROTECTED
- Challenge expiry bypass: PROTECTED (`WHERE expires_at > ?now` evaluated server-side)
- Scope confusion: PROTECTED (`WHERE scope = ?required_scope` in SQL; only `feed:write` accepted at challenge creation)

**Crypto researcher (tests/crypto_security_tests.rs -- 18 tests):**
- 10 attack vectors tested
- Ed25519 signatures correctly cover all fields (event_id, event_type, payload_json, subject_guid, created_at)
- Signing payload serialization is deterministic with declaration-order field serialization
- Token binding uses 128-bit CSPRNG entropy; nonce reuse still produces unique bindings
- Base64url encoding eliminates dot-separator ambiguity in `recompute_binding`
- Event IDs are UUIDv4 (122 bits entropy) with PRIMARY KEY constraint preventing duplicates
- Community node push filtering: pubkey check + ed25519 verification (defense-in-depth)
- Historical v1 finding closed: proof-of-possession now fetches the RSS feed and verifies the `podcast:txt` token binding before issuing write tokens. See [docs/security/crypto-blackbox-report-v2.md](crypto-blackbox-report-v2.md) for the current assessment.

**Availability researcher (tests/availability_tests.rs -- 12 tests):**
- 7 DoS vectors identified and remediated
- AVAIL-01 (HIGH): Proof challenge table exhaustion -- fixed with per-feed cap of 20 pending challenges, returns 429
- AVAIL-02 (MEDIUM): No explicit body size limit -- fixed with explicit `DefaultBodyLimit::max(2 MiB)` on all routers
- AVAIL-03 (MEDIUM): FTS5 search index bomb -- fixed with `truncate_fts_field()` capping text at 10,000 bytes
- AVAIL-04 (MEDIUM): Unbounded track count per ingest -- fixed with `MAX_TRACKS_PER_INGEST = 500`
- AVAIL-05 (MEDIUM): Unbounded push event array -- fixed with `MAX_PUSH_EVENTS = 1,000`
- AVAIL-06 (LOW): Unbounded reconcile have array -- fixed with `MAX_RECONCILE_HAVE = 10,000`
- AVAIL-07 (LOW): Oversized requester nonce -- fixed with `MAX_NONCE_BYTES = 256`

---

### Remaining Known Gaps (for future work)

- **Proof-of-possession feed verification (podcast:txt):** Implemented. This report originally called it out as the critical open gap; that finding is now closed and documented in `crypto-blackbox-report-v2.md` and `auth-blackbox-report-v2.md`.

- **Admin token constant-time comparison:** The `check_admin_token` function uses `==` (non-constant-time). Switching to `subtle::ConstantTimeEq` or `ring::constant_time::verify_slices_are_equal` would be a defense-in-depth improvement. Not practically exploitable given network jitter and token entropy.

- **Structured logging (tracing crate):** The codebase uses `eprintln!` and `println!` for diagnostics. Migrating to the `tracing` crate would provide structured, level-filtered, span-aware logging suitable for production observability.

- **Connection limits for SSE push:** The sync register endpoint (`POST /sync/register`) is unauthenticated. While the peer eviction mechanism limits blast radius, adding authentication or connection limits would harden the push fanout system as the network grows.

---

### Test Coverage Summary

- **Total test count:** 247 passed, 0 failed, 1 ignored (doc-test with `ignore` attribute)
- **New tests added in this sprint cycle:**
  - `auth_atomicity_tests.rs`: 11 tests (Sprint 1A)
  - `rfc6750_tests.rs`: 12 tests (Sprint 1B)
  - `sprint2a_tests.rs`: 9 tests (Sprint 2A)
  - `sprint2b_tests.rs`: 8 tests (Sprint 2B)
  - `proof_tests.rs`: 22 tests (Sprint 3A, expanded from initial set)
  - `tls_tests.rs`: 8 tests (Sprint 3B)
  - `retire_remove_tests.rs`: 10 tests (Sprint 3C)
  - `sprint4a_tests.rs`: 5 tests (Sprint 4A)
  - `security_auth_tests.rs`: 17 tests (auth blackbox)
  - `crypto_security_tests.rs`: 18 tests (crypto blackbox)
  - `availability_tests.rs`: 12 tests (availability blackbox)
