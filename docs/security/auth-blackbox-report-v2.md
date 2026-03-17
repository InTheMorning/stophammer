# Authentication Security Audit Report v2

**Date:** 2026-03-13
**Scope:** Re-audit of all v1 findings plus new attack surfaces introduced since v1
**Files Audited:**
- `src/api.rs` (authentication, handlers, SSE registry, CORS, rate limiter config)
- `src/proof.rs` (challenge/token lifecycle, verify_podcast_txt, is_podcast_txt_element)
- `src/main.rs` (rate limiter middleware, X-Forwarded-For extraction)
- `src/db.rs` (delete_feed_with_event cascade including SG-07 proof cleanup)

**Test file:** `tests/security_auth_v2_tests.rs` (27 tests, all passing)

---

## Executive Summary

This is a follow-up audit to `auth-blackbox-report.md` (v1, 2026-03-12). Significant security improvements have been implemented since v1:

1. **CS-01 (RSS verification):** `handle_proofs_assert` now performs a three-phase flow that fetches the feed RSS and verifies `podcast:txt` before issuing tokens.
2. **CS-02 (constant-time admin token):** Admin token comparison now uses SHA-256 + `subtle::ConstantTimeEq`, closing the v1 timing side-channel.
3. **CS-03 (authenticated sync/register):** `POST /sync/register` now requires `X-Admin-Token`.
4. **SG-07 (proof cleanup on feed delete):** Both `proof_tokens` and `proof_challenges` are cascade-deleted when a feed is retired.
5. **SP-05 (epoch guard):** `SystemTime::now()` failure now panics via `.expect()` instead of silently returning epoch 0.

All 7 original v1 findings are now CLOSED. Four new vulnerabilities were identified across the new attack surfaces and all have been remediated:
- **SSRF via feed_url** -- `validate_feed_url` guard added to `handle_proofs_assert` with private-IP and scheme validation
- **TOCTOU double token issue** -- `resolve_challenge` now returns rows-affected count; Phase 3 rejects if 0 rows transitioned
- **X-Forwarded-For rate limiter bypass** -- `TRUST_PROXY` config added; default ignores XFF to prevent spoofing
- **SSE unbounded memory** -- `artists` query parameter capped at 50 IDs per connection

---

## V1 Finding Re-Verification

### 1. Token Replay After Feed Deletion

**V1 Status:** PROTECTED (subject GUID binding prevented cross-feed replay; orphaned tokens were harmless)
**V2 Status:** CLOSED

SG-07 implemented. `delete_feed_with_event` (`db.rs` line 998-1000) now cascade-deletes both `proof_tokens` and `proof_challenges` for the deleted feed within the same transaction. The orphaned token problem is fully eliminated -- tokens are physically removed, not just logically inert.

**Evidence:** Test `v2_attack1_token_cascade_deleted_on_feed_delete` verifies that `validate_token` returns `None` after feed deletion, and the subsequent PATCH returns 401 (not 403).

---

### 2. Challenge Race Condition (Double-Spend)

**V1 Status:** PROTECTED
**V2 Status:** CLOSED (unchanged)

The SQLite mutex + `WHERE state = 'pending'` idempotent update still prevents double-spending of challenges. The new three-phase architecture introduces a theoretical TOCTOU window (see NEW-3 below), but the core double-spend protection is intact.

**Evidence:** Test `v2_attack2_resolve_challenge_still_idempotent`

---

### 3. Cross-Feed Token

**V1 Status:** PROTECTED
**V2 Status:** CLOSED (unchanged)

`check_admin_or_bearer_with_conn` (`api.rs` line 1810) still enforces `subject != expected_feed_guid`, rejecting tokens scoped to a different feed.

**Evidence:** Test `v2_attack3_cross_feed_token_still_rejected`

---

### 4. Admin Token Timing Attack

**V1 Status:** PARTIALLY_PROTECTED (non-constant-time `==` comparison)
**V2 Status:** CLOSED (CS-02 implemented)

`check_admin_token` (`api.rs` lines 1339-1362) now:
1. Hashes both the provided and expected tokens with SHA-256
2. Compares the hashes using `subtle::ConstantTimeEq` (`ct_eq`)

This eliminates the timing side-channel entirely. The SHA-256 pre-hash ensures equal-length inputs to `ct_eq`, and `ct_eq` performs constant-time byte comparison.

```rust
let h1 = Sha256::digest(provided.as_bytes());
let h2 = Sha256::digest(expected.as_bytes());
if bool::from(h1.ct_eq(&h2)) { Ok(()) }
```

Additionally, an empty `admin_token` on the server now returns `Err` immediately (`api.rs` line 1340-1346), preventing accidental admin access when the token is misconfigured.

**Evidence:** Test `v2_attack4_admin_token_now_constant_time`

---

### 5. Bearer Token Format Bypass

**V1 Status:** PROTECTED
**V2 Status:** CLOSED (unchanged)

`extract_bearer_token` continues to reject empty, whitespace-only, and non-Bearer-scheme tokens.

**Evidence:** Test `v2_attack5_bearer_format_bypass_still_protected`

---

### 6. Challenge Expiry Bypass

**V1 Status:** PROTECTED
**V2 Status:** CLOSED (unchanged)

`get_challenge` (`proof.rs` line 103) still uses `WHERE expires_at > ?now`, and SP-05 ensures `now_secs()` never returns 0.

**Evidence:** Tests `v2_attack6_expired_challenge_still_returns_none` and `v2_sp05_unix_now_returns_sane_value`

---

### 7. Scope Confusion

**V1 Status:** PROTECTED
**V2 Status:** CLOSED (unchanged)

`validate_token` still uses `WHERE scope = ?required_scope` for exact scope matching. Only `"feed:write"` is accepted at challenge creation.

**Evidence:** Test `v2_attack7_scope_confusion_still_rejected`

---

## New Attack Surface Analysis

### NEW-1: CS-01 RSS Verification Bypass

#### NEW-1a: Attacker-Controlled Server Serves Matching podcast:txt

**Finding: BY_DESIGN (not a vulnerability)**

If the feed_url points to an attacker-controlled server, the attacker can serve the correct `podcast:txt` on demand and obtain a token. This is the intended behavior -- the podcast:txt proof model is fundamentally based on "whoever controls the feed URL is the feed owner." If an attacker controls the DNS/hosting of the feed URL, they ARE the legitimate owner from the system's perspective.

The feed_url is set during crawler ingest and stored in the feeds table. An attacker would need either:
- A valid crawl token to ingest a feed with their URL (requires crawler access)
- A bearer token scoped to the feed to PATCH the feed_url (requires prior proof-of-possession)
- The admin token to PATCH the feed_url

None of these represent a bypass of the authentication system.

**Evidence:** Test `v2_cs01_attacker_controlled_server_is_by_design`

#### NEW-1b: podcast:txt Parse Ambiguity (Extra Content)

**Finding: PROTECTED**

The `verify_podcast_txt` function (`proof.rs` line 302) uses exact trimmed equality:
```rust
if trimmed == expected_text {
```

This rejects:
- Prefix injection: `"INJECTED stophammer-proof token.hash"` does not match
- Suffix injection: `"stophammer-proof token.hash AND EXTRA"` does not match
- Only `"stophammer-proof token.hash"` (with optional whitespace trim) matches

**Evidence:** Tests `v2_cs01_podcast_txt_partial_match_rejected` and `v2_cs01_podcast_txt_prefix_attack_rejected`

#### NEW-1c: SSRF via feed_url

**Finding: FIXED (was Medium)**

**Severity:** Medium (pre-fix)
**Attack:** An attacker who obtains write access to a feed (via proof-of-possession or admin token) could `PATCH /v1/feeds/{guid}` to change `feed_url` to an internal URL (e.g., `http://127.0.0.1:8080/admin`, `http://169.254.169.254/latest/meta-data/`). When a subsequent `POST /v1/proofs/assert` was made for that feed, `verify_podcast_txt` would fetch the attacker-specified URL, acting as an SSRF proxy.

**Fix implemented:** `validate_feed_url` (`proof.rs` lines 358-394) is now called in `handle_proofs_assert` Phase 2 before the RSS fetch. It rejects:
- Non-HTTP(S) schemes (`file://`, `ftp://`, etc.)
- Literal private/reserved IP addresses (loopback, RFC1918, link-local, broadcast, unspecified, CGNAT)
- Hostnames that resolve to private/reserved IP ranges (DNS resolution check)
- IPv6 private ranges (loopback `::1`, unique-local `fc00::/7`, link-local `fe80::/10`)

The guard is placed at the API layer via `AppState.skip_ssrf_validation` flag (set to `false` in production, `true` in tests that use wiremock's localhost-bound mock servers).

**Evidence:** Tests `v2_cs01_validate_feed_url_rejects_private_ips`, `v2_cs01_validate_feed_url_rejects_disallowed_schemes`, `v2_cs01_validate_feed_url_rejects_file_scheme`, `v2_cs01_validate_feed_url_accepts_public_urls`, `v2_cs01_ssrf_blocked_at_api_layer`, `v2_cs01_assert_with_localhost_feed_url_blocked_by_ssrf`

---

### NEW-2: CS-03 Admin Token as Single Point of Failure

**Finding: ACCEPTED_RISK**

**Severity:** Informational (architectural)

The admin token is the sole credential for all privileged operations:
- `POST /sync/register` (register push peers)
- `POST /admin/artists/merge` (merge artists)
- `POST /admin/artists/alias` (add aliases)
- `DELETE /v1/feeds/{guid}` (delete feeds, as alternative to bearer token)
- `PATCH /v1/feeds/{guid}` and `PATCH /v1/tracks/{guid}` (as alternative to bearer)

If compromised, an attacker gains full read-write access to the catalog. This is architecturally equivalent to a database password -- it is a server-side secret, not a user-facing credential.

**Mitigating factors:**
- The admin token is set via environment variable, not stored in the DB
- CS-02's constant-time comparison prevents timing attacks to extract it
- SP-03 rate limiting constrains brute-force attempts (though XFF spoofing weakens this, see NEW-4a)
- TLS encrypts the token in transit

**Recommendation:** For defense-in-depth, consider:
- Separate admin tokens for different privilege levels (sync-admin vs catalog-admin)
- Short-lived admin sessions with token rotation
- Audit logging for all admin operations

**Evidence:** Test `v2_cs03_admin_token_blast_radius`

---

### NEW-3: Three-Phase TOCTOU in handle_proofs_assert

**Finding: FIXED (was Low)**

**Severity:** Low (pre-fix)
**Attack:** `handle_proofs_assert` uses a three-phase architecture:

| Phase | Lock | Operation |
|-------|------|-----------|
| Phase 1 | Held | Validate nonce, load challenge (must be "pending"), look up feed_url |
| Phase 2 | Released | Async RSS fetch + verify podcast:txt |
| Phase 3 | Re-acquired | resolve_challenge + issue_token |

The DB mutex is released between Phase 1 and Phase 3. During Phase 2 (RSS fetch, up to 10 seconds), the challenge remains in "pending" state. A second concurrent request could enter Phase 3 and issue a duplicate token.

**Fix implemented:** `resolve_challenge` (`proof.rs` lines 78-89) now returns `usize` (rows affected) instead of `()`. In Phase 3, `handle_proofs_assert` checks:
```rust
let rows = proof::resolve_challenge(&conn, &challenge_id, "valid")?;
if rows == 0 {
    return Err(ApiError { status: 400, message: "challenge already resolved (concurrent request)" });
}
```

If a concurrent request already resolved the challenge, the UPDATE's `WHERE state = 'pending'` clause matches zero rows, and the second request gets HTTP 400 instead of a duplicate token.

**Evidence:** Test `v2_cs01_toctou_now_fixed_via_rows_check`

---

### NEW-4: Rate Limiter Vulnerabilities

#### NEW-4a: X-Forwarded-For Spoofing

**Finding: FIXED (was Medium, deployment-dependent)**

**Severity:** Medium (pre-fix, when directly exposed)

The rate limiter previously unconditionally preferred `X-Forwarded-For` over `ConnectInfo`, allowing attackers to spoof IPs and bypass rate limiting.

**Fix implemented:** `apply_rate_limit` (`main.rs` lines 215-271) now respects the `TRUST_PROXY` environment variable:
- **`TRUST_PROXY=true` (or `1`):** Uses `X-Forwarded-For` (first hop) with `ConnectInfo` fallback. This is correct when deployed behind a trusted reverse proxy (e.g., nginx, Cloudflare).
- **Default (no `TRUST_PROXY`):** Ignores `X-Forwarded-For` entirely and uses only `ConnectInfo<SocketAddr>`. This prevents XFF spoofing when the server is directly exposed.

**Evidence:** Tests `v2_rate_limiter_xff_spoofing` and `v2_rate_limiter_unknown_fallback_shared_bucket`

#### NEW-4b: "unknown" Fallback Shared Bucket

**Finding: INFORMATIONAL**

When neither `X-Forwarded-For` nor `ConnectInfo` is available, all requests share the key `"unknown"`. In practice, `ConnectInfo` is always available for TCP connections (via `make_service_with_connect_info`), so this fallback only triggers in test contexts. Not a real-world concern.

**Evidence:** Test `v2_rate_limiter_unknown_fallback_shared_bucket`

---

### NEW-5: SSE Registry Unbounded Memory Growth

**Finding: PARTIALLY_FIXED (was Low)**

**Severity:** Low (availability only, no auth bypass)

The `SseRegistry` creates a `tokio::sync::broadcast` channel per unique `artist_id` on `subscribe()`. Without a cap, an attacker could create unlimited channels.

**Fix implemented:** `handle_sse_events` (`api.rs`) now caps the `artists` query parameter to `MAX_SSE_ARTISTS = 50` IDs per connection using `.take(MAX_SSE_ARTISTS)`. This limits the per-request blast radius.

**Remaining risk:** The total number of distinct artist_ids in the `senders` map is still unbounded across requests. TTL-based eviction for channels with no active subscribers and/or an LRU cache would provide complete protection.

**Evidence:** Test `v2_sse_unlimited_artist_registrations` confirms the per-connection cap is enforced.

### NEW-5b: SSE Cross-Pollination (Information Leak)

**Finding: PROTECTED**

Each artist_id has an independent broadcast channel. Subscribing to artist-A never delivers events for artist-B.

**Evidence:** Test `v2_sse_no_cross_pollination`

---

### NEW-6: SG-07 Proof Cleanup on Feed Delete

**Finding: CLOSED**

Both `proof_tokens` and `proof_challenges` are now deleted in the `delete_feed_with_event` transaction (`db.rs` lines 998-1000). This prevents:
- Orphaned tokens surviving feed deletion
- Orphaned challenges being asserted after feed re-creation with different ownership

**Evidence:** Tests `v2_attack1_token_cascade_deleted_on_feed_delete` and `v2_sg07_challenges_deleted_on_feed_delete`

---

### NEW-7: CORS Configuration

**Finding: INFORMATIONAL**

`build_cors_layer` (`api.rs` lines 355-371) uses `allow_origin(Any)`, meaning any web origin can make cross-origin requests. The allowed headers include `Authorization`, `Content-Type`, and `X-Admin-Token`.

Exposing `X-Admin-Token` in CORS `allow_headers` means browser JavaScript from any origin could make admin requests if it knows the token. In practice, the admin token should never be available to browser code (it is a server-side secret).

**Recommendation:** Consider removing `x-admin-token` from CORS `allow_headers` if admin operations are never performed from browsers. This adds a defense-in-depth layer against XSS-based admin token exfiltration.

---

### NEW-8: Pending Challenge Flooding

**Finding: PROTECTED**

`handle_proofs_challenge` (`api.rs` lines 1878-1892) enforces a maximum of 20 pending challenges per `feed_guid` (`MAX_PENDING_CHALLENGES_PER_FEED`). Exceeding this limit returns HTTP 429.

**Evidence:** Test `v2_challenge_flooding_capped_per_feed`

---

## Summary Table

### V1 Finding Re-Verification

| # | Attack Vector | V1 Status | V2 Status | Fix Applied |
|---|--------------|-----------|-----------|-------------|
| 1 | Token replay after feed deletion | PROTECTED | CLOSED | SG-07 cascade delete |
| 2 | Challenge race condition | PROTECTED | CLOSED | Unchanged |
| 3 | Cross-feed token | PROTECTED | CLOSED | Unchanged |
| 4 | Admin token timing attack | PARTIALLY_PROTECTED | CLOSED | CS-02 SHA-256 + ct_eq |
| 5 | Bearer token format bypass | PROTECTED | CLOSED | Unchanged |
| 6 | Challenge expiry bypass | PROTECTED | CLOSED | Unchanged + SP-05 |
| 7 | Scope confusion | PROTECTED | CLOSED | Unchanged |

### New Attack Surface Findings

| # | Attack Surface | Finding | Severity | Status |
|---|---------------|---------|----------|--------|
| NEW-1a | Attacker-controlled RSS server | BY_DESIGN | N/A | No fix needed |
| NEW-1b | podcast:txt parse ambiguity | PROTECTED | N/A | No fix needed |
| NEW-1c | SSRF via feed_url | FIXED | Medium | `validate_feed_url` guard |
| NEW-2 | Admin token single point of failure | ACCEPTED_RISK | Informational | Architectural |
| NEW-3 | Three-phase TOCTOU double token issue | FIXED | Low | rows-affected check |
| NEW-4a | X-Forwarded-For rate limiter bypass | FIXED | Medium | `TRUST_PROXY` config |
| NEW-4b | "unknown" fallback shared bucket | INFORMATIONAL | None | No fix needed |
| NEW-5 | SSE registry unbounded memory growth | PARTIALLY_FIXED | Low | Per-connection cap (50) |
| NEW-5b | SSE cross-pollination | PROTECTED | N/A | No fix needed |
| NEW-6 | Proof cleanup on feed delete | CLOSED | N/A | No fix needed |
| NEW-7 | CORS X-Admin-Token exposure | INFORMATIONAL | Low | Optional |
| NEW-8 | Challenge flooding | PROTECTED | N/A | No fix needed |

---

## Remediation Status

All critical and medium findings have been fixed in this audit cycle:

| Priority | Finding | Status | Implementation |
|----------|---------|--------|----------------|
| 1 | NEW-1c (SSRF) | DONE | `validate_feed_url` in `proof.rs`, called from `handle_proofs_assert` |
| 2 | NEW-4a (XFF spoofing) | DONE | `TRUST_PROXY` env var in `apply_rate_limit` (`main.rs`) |
| 3 | NEW-3 (TOCTOU) | DONE | `resolve_challenge` returns `usize`; Phase 3 checks rows == 0 |
| 4 | NEW-5 (SSE memory) | PARTIAL | Per-connection cap of 50 artists; global TTL eviction still recommended |
| 5 | NEW-7 (CORS) | OPEN | Optional: remove `x-admin-token` from CORS `allow_headers` |

### Remaining Recommendations

1. **SSE global eviction:** Add TTL-based eviction for `SseRegistry` channels with no active subscribers to prevent slow-drip memory growth across many requests.
2. **CORS hardening:** Remove `x-admin-token` from CORS `allow_headers` if admin operations are never performed from browsers.
3. **Admin token rotation:** Consider short-lived admin sessions or separate tokens for different privilege levels (defense-in-depth).
