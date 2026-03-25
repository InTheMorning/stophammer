# Cryptographic Security Black-Box Report v2

**Date:** 2026-03-25
**Scope:** Re-audit of all v1 findings plus follow-up verification against the current tree
**Files audited:**
- `src/proof.rs` -- `verify_podcast_txt`, `validate_feed_url`, `recompute_binding`, SSRF guard
- `src/api.rs` -- `handle_proofs_assert` (3-phase RSS verification), `check_admin_token` (CS-02 constant-time), SSE event stream, tracing
- `src/search.rs` -- `SipHasher24` FTS5 rowid computation
- `src/signing.rs` -- Ed25519 (unchanged from v1)
- `src/community.rs` -- push registration, tracing audit
- `src/main.rs` -- tracing audit
- `Cargo.toml` -- dependency versions

**Previous report:** `crypto-blackbox-report.md` (v1, 2026-03-12)

---

## Executive Summary

The critical v1 finding (Finding #8 -- proof-of-possession bypass) has been **closed**. The `handle_proofs_assert` handler now fetches the feed's RSS and verifies that a `<podcast:txt>` element contains the exact `stophammer-proof <token_binding>` string before issuing a write token. SSRF protections are in place. The TOCTOU race between RSS verification and token issuance is mitigated by the `WHERE state = 'pending'` guard in `resolve_challenge`.

All other v1 findings remain CLOSED (no regressions).

The current tree closes the critical v1 proof bypass and now also hardens the
RSS verification path with bounded response bodies, redirect re-validation, and
DNS pinning across the fetch chain. SSE events remain unsigned by design, which
is acceptable for their notification-only role. No new critical or high-severity
vulnerabilities were identified.

---

## Re-verification of v1 Findings

### Finding #8 (v1: CRITICAL/VULNERABLE): Proof-of-Possession Bypass

**v2 Status: CLOSED**

The `handle_proofs_assert` handler (api.rs lines 1951-2112) now implements a three-phase flow:

1. **Phase 1 (blocking):** Validates nonce, loads challenge, looks up `feed_url` from the `feeds` table via `db::get_feed_by_guid`.
2. **Phase 2 (async):** Calls `proof::validate_feed_url` (SSRF guard), then `proof::verify_podcast_txt` to fetch RSS and verify `podcast:txt`.
3. **Phase 3 (blocking):** If RSS verified, calls `resolve_challenge` with `WHERE state = 'pending'` to atomically transition the challenge and issue the token.

**Sub-finding 8a: Does `verify_podcast_txt` correctly parse `podcast:txt`?**

Yes. The function (proof.rs lines 244-317) uses `quick_xml::Reader` to parse the RSS body as a streaming XML parser. It tracks `<channel>` depth and only considers `podcast:txt` elements inside `<channel>`. The `is_podcast_txt_element` helper (proof.rs lines 326-344) matches either the literal `podcast:txt` tag name or any `prefix:txt` where the prefix is declared with `xmlns:<prefix>="https://podcastindex.org/namespace/1.0"` in the document. This correctly handles both standard and non-standard namespace prefixes.

**Sub-finding 8b: Is the match exact or can partial matching bypass it?**

PROTECTED. The comparison at proof.rs line 304 is:
```
if trimmed == expected_text {
```
This is an exact string equality check after `.trim()`. The `expected_text` is `"stophammer-proof {token_binding}"` where `token_binding` is the full binding string (e.g., `"AbCdEf123.XyZ789hash"`). A partial match like `"stophammer-proof ATOK"` when the actual binding is `"stophammer-proof ATOKEN"` will NOT match because `==` requires full equality. No substring or prefix matching is used.

**Sub-finding 8c: TOCTOU between RSS verification and token issuance?**

PROTECTED. In Phase 3, `resolve_challenge` executes:
```sql
UPDATE proof_challenges SET state = ?1 WHERE challenge_id = ?2 AND state = 'pending'
```
The `AND state = 'pending'` clause is atomic at the SQLite level. If a concurrent request resolves the same challenge between Phase 1 and Phase 3, the `rows` count will be 0 and the handler returns 400 "challenge already resolved (concurrent request)" (api.rs lines 2081-2087). This prevents double-issuance.

There is a theoretical window where: (1) Request A verifies RSS successfully in Phase 2, (2) the feed owner removes the `podcast:txt` tag, (3) Request A proceeds to Phase 3 and issues the token. This is inherent to any challenge-response system with asynchronous verification and is not practically exploitable -- the feed owner placed the tag voluntarily.

**Sub-finding 8d: Malformed XML handling?**

PROTECTED. The `quick_xml::Reader` returns `Err(e)` for malformed XML, which is caught at proof.rs line 311 and returned as `Err(format!("RSS parse error: {e}"))`. The `t.unescape().unwrap_or_default()` at line 302 prevents panic on malformed XML entities (falls back to empty string). The `depth.saturating_sub(1)` at line 299 prevents underflow.

**Sub-finding 8e: Can the attacker control `feed_url`?**

PROTECTED. The `feed_url` used for RSS verification is read from the `feeds` table via `db::get_feed_by_guid` (api.rs line 2010). This URL is set during feed ingest by the crawler, not by the feed owner. The `PATCH /v1/feeds/{guid}` endpoint can update `feed_url`, but it requires either admin token or a bearer token scoped to that feed (api.rs line 2139) -- creating a chicken-and-egg protection: you need a token to change the URL, but you need the URL to point to your RSS to get a token.

Additionally, `proof::validate_feed_url` (proof.rs lines 358-394) implements SSRF protection:
- Rejects non-HTTP(S) schemes
- Resolves hostnames and checks all resolved IPs against private/reserved ranges
- Checks literal IP addresses in the hostname
- Covers IPv4 (loopback, private, link-local, broadcast, unspecified, CGNAT 100.64.0.0/10) and IPv6 (loopback, unspecified, unique-local fc00::/7, link-local fe80::/10)

The current implementation closes the earlier rebinding gap by resolving the
initial hostname up front, validating those addresses, and then manually
following redirects with per-hop validation and DNS pinning. The TCP
connections are therefore made against the same validated addresses rather than
against a second independent DNS lookup.

---

### Findings #1-7, #9-10 (v1: all PROTECTED)

| # | Attack Vector | v1 Finding | v2 Status |
|---|---|---|---|
| 1 | Signature Forgery | PROTECTED | CLOSED (no regression) |
| 2 | Nonce Reuse | PROTECTED | CLOSED (no regression) |
| 3 | Token Binding Malleability | PROTECTED | CLOSED (no regression) |
| 4 | Event ID Collision | PROTECTED | CLOSED (no regression) |
| 5 | Signed Payload Injection | PROTECTED | CLOSED (no regression) |
| 6 | Content Hash Collision | PROTECTED | CLOSED (no regression) |
| 7 | Base64 Decoding Attacks | PROTECTED | CLOSED (no regression) |
| 9 | Challenge Replay | PROTECTED | CLOSED (no regression) |
| 10 | Community Node Filtering | PROTECTED | CLOSED (no regression) |

No code changes were made to the Ed25519 signing/verification pipeline, event ID generation, or challenge replay protection. The `signing.rs` and `proof.rs` core functions (create_challenge, resolve_challenge, recompute_binding) are unchanged from v1.

---

## New Attack Surface Analysis

### N1: SipHash FTS5 Rowid Collisions

**Finding: ACCEPTABLE RISK (Low)**

`search.rs::rowid_for` uses `SipHasher24` with fixed keys `(0, 0)` to compute FTS5 rowids from `entity_type + "\0" + entity_id` (search.rs lines 53-61). The hash is masked to 63 bits (positive i64).

**Analysis:**

Because the keys are fixed and public (zero), an attacker who knows the algorithm can pre-compute inputs that produce colliding rowids. If two entities hash to the same rowid:

1. The `populate_search_index` function first deletes the existing row by rowid, then inserts a new one (search.rs lines 91-105).
2. If Entity A and Entity B have the same rowid, indexing B would first delete A's search entry (via the FTS5 `'delete'` command), then insert B. Entity A would become unsearchable.

**Exploitability:** Low. The attacker would need to craft an `entity_type + entity_id` pair that collides with a target entity's rowid. The entity_id for feeds is a podcast GUID (from the RSS), and for tracks is also a GUID. The attacker cannot freely choose entity IDs -- they are derived from RSS feed content during ingest. Additionally, finding a SipHash-2-4 collision with fixed keys for a specific target requires brute-forcing approximately 2^63 / N inputs (where N is the number of existing entities), which is computationally expensive.

**Impact:** Search index corruption (entity disappears from search results). Not a security breach -- no data is modified, no authentication bypassed. The entity still exists in the database and can be accessed by direct GUID lookup.

**Recommendation:** No fix needed. If collision resistance is desired in the future, the fixed keys could be replaced with random keys generated at database creation time and stored in a metadata table. This would make pre-computation impossible while maintaining stability across process restarts.

### N2: Constant-Time Admin Token Comparison (CS-02)

**Finding: PROTECTED (defense-in-depth)**

`check_admin_token` (api.rs lines 1348-1371) computes `SHA-256(provided)` and `SHA-256(expected)`, then uses `subtle::ConstantTimeEq::ct_eq` to compare the two 32-byte digests.

**Analysis:**

The question raised is whether SHA-256-then-ct_eq is meaningfully more secure than ct_eq directly on raw bytes.

Both approaches achieve the primary goal: preventing timing side-channel leaks of the token value. The SHA-256 pre-hashing adds one property: the comparison always operates on fixed-length 32-byte inputs regardless of token length, which eliminates any length-dependent timing variations that could occur in a naive comparison. However, `subtle::ConstantTimeEq` on byte slices already handles length comparison in constant time (returns false for mismatched lengths without revealing which bytes differ).

The SHA-256 approach does NOT add meaningful security against an attacker who has obtained the hash -- SHA-256 is a one-way function, so the hash does not reveal the token. But an attacker who could observe the hash (e.g., from memory dumps) would need to invert SHA-256 to recover the admin token, which is the same difficulty as brute-forcing the token directly.

**Verdict:** The current implementation is correct and provides defense-in-depth. Using ct_eq directly on raw bytes would be equally secure for the timing side-channel threat. The SHA-256 pre-hashing is a minor over-engineering but causes no harm.

### N3: Token Binding Replay with Old Nonce

**Finding: PROTECTED**

**Analysis of `recompute_binding` (proof.rs lines 187-196):**

The function takes a `stored_binding` and a `requester_nonce`, splits the stored binding on `.` to extract the `base_token`, then recomputes `base_token + "." + base64url(SHA-256(requester_nonce))`.

**Can an old nonce replay a different challenge?**

No. Each challenge generates a unique random `base_token` (128 bits from OsRng). The `token_binding` stored in the DB is `unique_base_token.hash(nonce)`. During assertion:
1. The server loads the challenge by `challenge_id` (UUID, unguessable)
2. It calls `recompute_binding(challenge.token_binding, requester_nonce)`
3. It compares the recomputed binding with the stored `challenge.token_binding`

Even if an attacker reuses the same nonce from a previous challenge, the `base_token` portion differs (it was generated fresh for each challenge). The recomputed binding would use the old nonce's hash but with the current challenge's base_token, which must match the stored binding. Since the stored binding already contains the correct hash of the nonce used at challenge-creation time, the nonce must be the same one used when the challenge was created.

Cross-challenge replay is impossible because:
- `challenge_id` is a UUID (122-bit entropy, unguessable)
- Each challenge has its own unique `base_token`
- `resolve_challenge` uses `WHERE state = 'pending'` for single-use enforcement

### N4: SSE Event Stream Integrity

**Finding: ACCEPTABLE RISK (Informational)**

**Analysis:**

SSE events (api.rs lines 1244-1343) are **not signed**. The `SseFrame` struct contains `event_type`, `subject_guid`, and `payload` (as `serde_json::Value`). Events are broadcast via `tokio::sync::broadcast` channels and serialized as JSON in the SSE data field.

**Can a malicious event be injected?**

An attacker cannot inject events into the SSE stream because:
1. The `SseRegistry::publish` method is only called internally by the server (not exposed via any HTTP endpoint).
2. SSE is a server-to-client protocol -- clients receive events but cannot send them back through the SSE connection.
3. The broadcast channels are in-process Tokio channels, not network-accessible.

**However**, SSE events are not cryptographically authenticated to the client. A network-level MITM (on a non-TLS connection) could inject or modify SSE frames. The main.rs TLS warning (line 325) already addresses this: "Bearer tokens and crawl tokens are transmitted unencrypted."

**Recommendation:** SSE events are real-time notifications (e.g., "a new track was added for artist X"). They do not carry authorization or mutation semantics. Signing each SSE frame would add computational overhead without meaningful security benefit -- the client should always verify state by querying the authenticated REST API before acting on an SSE notification. The current design is appropriate.

### N5: Tracing / Sensitive Data Logging

**Finding: PROTECTED**

**Audit of all `tracing::` calls across the codebase:**

| File | Line(s) | Fields Logged | Sensitive Data? |
|---|---|---|---|
| api.rs:1058 | "fanout: RwLock poisoned" | (none) | No |
| api.rs:1094-1101 | fanout push warnings | `url`, `attempt`, `status`, `error` | No |
| api.rs:1114-1118 | push success/failure | `peer` (pubkey hex) | No (public key) |
| api.rs:1136-1140 | push failure tracking | `peer`, `error` | No |
| api.rs:1156 | peer eviction | `peer`, `threshold` | No |
| api.rs:1200 | push peer registered | `peer` (pubkey), `url` | No |
| api.rs:1324 | SSE lagged | `lagged` (count) | No |
| community.rs:117-278 | sync, poll, registration | `primary`, `cursor`, `error`, `tracker`, `status` | No |
| community.rs:343 | rejected event | `event_id`, `signed_by` | No (public key) |
| community.rs:366 | apply summary | `applied`, `duplicate`, `rejected` | No |
| search.rs:98 | FTS5 pre-delete note | `entity_type`, `entity_id`, `error` | No |
| main.rs:191-197 | proof pruner | `pruned`, `error` | No |
| main.rs:318-328 | bind address, TLS mode | `bind` | No |
| apply.rs:232-248 | signature rejection, DB error | `event_id`, `seq`, `error` | No |
| verify.rs:212 | unknown verifier | `verifier` | No |
| tls.rs:175-271 | TLS provisioning | `domain`, `error` | No |

**No tracing statement logs:**
- Admin tokens or bearer tokens
- Access tokens or nonce values
- Signing keys or private keys
- Challenge IDs or token bindings
- Request bodies or authorization headers

The tracing implementation is clean. No sensitive data is logged at any level (error, warn, info, debug).

---

## New Observation: RSS Response Body Size Limit

**Finding: FIXED**

The current RSS proof fetch path enforces `MAX_RSS_BODY_BYTES = 5 MiB` and
checks both `Content-Length` and the streamed body size before accepting the
response. This closes the earlier memory-exhaustion concern for
`POST /v1/proofs/assert`.

---

## Summary Table

### v1 Finding Re-verification

| # | Attack Vector | v1 Finding | v2 Status |
|---|---|---|---|
| 1 | Signature Forgery | PROTECTED | CLOSED |
| 2 | Nonce Reuse | PROTECTED | CLOSED |
| 3 | Token Binding Malleability | PROTECTED | CLOSED |
| 4 | Event ID Collision | PROTECTED | CLOSED |
| 5 | Signed Payload Injection | PROTECTED | CLOSED |
| 6 | Content Hash Collision | PROTECTED | CLOSED |
| 7 | Base64 Decoding Attacks | PROTECTED | CLOSED |
| 8 | Proof-of-Possession Bypass | **VULNERABLE** | **CLOSED** |
| 9 | Challenge Replay | PROTECTED | CLOSED |
| 10 | Community Node Filtering | PROTECTED | CLOSED |

### New Attack Surfaces

| # | Attack Surface | Finding | Severity |
|---|---|---|---|
| N1 | SipHash FTS5 Rowid Collisions | ACCEPTABLE RISK | Low |
| N2 | Constant-Time Admin Token (CS-02) | PROTECTED | N/A |
| N3 | Token Binding Replay | PROTECTED | N/A |
| N4 | SSE Event Stream Integrity | ACCEPTABLE RISK | Informational |
| N5 | Tracing Sensitive Data Leakage | PROTECTED | N/A |
| N6 | RSS Response Body Size | FIXED | Low |

---

## Recommendations

1. **Monitor SipHash collision rates** -- If the search index grows to millions of entities, hash collisions become more likely (birthday bound at ~2^31.5 entities for 63-bit output). Consider logging when a pre-delete affects a different entity than expected.

2. **Document SSE trust model** -- SSE events are informational notifications, not authoritative state. Client applications should query the REST API to confirm state before acting on SSE notifications.
