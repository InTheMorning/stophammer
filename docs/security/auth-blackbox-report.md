# Authentication Security Audit Report

**Date:** 2026-03-12

> Historical snapshot. This report reflects the 2026-03-12 tree and is
> superseded by `auth-blackbox-report-v2.md` and `final-audit-report.md` for
> current behavior.
**Scope:** Proof-of-possession token system, admin token authentication, challenge/assert flow
**Files Audited:**
- `src/api.rs` (authentication functions, HTTP handlers)
- `src/proof.rs` (challenge/token lifecycle)
- `src/db.rs` (database operations, feed deletion cascades)
- `src/schema.sql` (table definitions, constraints)

**Test file:** `tests/security_auth_tests.rs` (17 tests, all passing)

---

## Executive Summary

The stophammer authentication system is well-designed with no exploitable vulnerabilities found in the tested attack classes. All 7 attack vectors were tested with concrete proof-of-concept code. The system benefits from several architectural decisions that provide defense in depth:

1. **Feed-scoped bearer tokens** with subject GUID validation on every request
2. **Single SQLite mutex** eliminating all concurrency-related TOCTOU attacks
3. **Parameterized SQL queries** preventing injection
4. **Scope validation** at both challenge creation and token usage
5. **Expiry checks** enforced at the query level (`WHERE expires_at > ?now`)

One defense-in-depth improvement is noted (admin token constant-time comparison) but does not represent a practically exploitable vulnerability.

---

## Attack Vectors Tested

### 1. Token Replay After Feed Deletion

**Finding: PROTECTED**

**Scenario:** An attacker obtains a bearer token for feed-A. Feed-A is then deleted via `DELETE /v1/feeds/{guid}`. The attacker attempts to use the surviving token to PATCH feed-B.

**Analysis:** The `proof_tokens` table has no foreign key referencing `feeds`, so tokens survive feed deletion (the `delete_feed_with_event` transaction in `db.rs` lines 926-990 does not clean up `proof_tokens`). However, the token remains bound to its original `subject_feed_guid`:

- `validate_token()` (`proof.rs` line 164-174) returns the `subject_feed_guid` stored in the token row
- `check_admin_or_bearer_with_conn()` (`api.rs` line 1436) compares `subject != expected_feed_guid`
- A token for feed-A can never authorize operations on feed-B

The orphaned token can still "authorize" requests targeting the deleted feed's GUID, but since the feed no longer exists, the SQL UPDATE affects 0 rows -- a harmless no-op.

**Evidence:** Tests `attack1_token_replay_after_feed_deletion` and `attack1b_orphaned_token_patch_deleted_feed_is_noop` both pass.

**Recommendation (minor):** Consider deleting tokens for a feed when the feed is retired. This is defense-in-depth only -- there is no exploitable path.

---

### 2. Challenge Race Condition (Double-Spend)

**Finding: PROTECTED**

**Scenario:** Two concurrent `POST /v1/proofs/assert` requests for the same `challenge_id` both succeed, issuing two tokens from a single challenge.

**Analysis:** This attack is impossible due to two layered defenses:

1. **SQLite + Rust Mutex:** The entire `handle_proofs_assert` handler acquires the DB mutex (`state2.db.lock()`) before calling `get_challenge` and `resolve_challenge`. Only one thread can execute the assert flow at a time.

2. **SQL-level idempotency:** `resolve_challenge()` (`proof.rs` line 82) uses `WHERE state = 'pending'` in the UPDATE. Once the state is changed to `'valid'`, subsequent calls match 0 rows and are no-ops.

3. **Handler-level state check:** `handle_proofs_assert` (`api.rs` line 1570) checks `challenge.state != "pending"` and returns 400 if already resolved.

**Evidence:** Tests `attack2_resolve_challenge_is_idempotent` and `attack2_double_assert_returns_400_on_second` both pass.

---

### 3. Cross-Feed Token

**Finding: PROTECTED**

**Scenario:** A token issued for feed-A is used to `PATCH /v1/feeds/feed-B`.

**Analysis:** `check_admin_or_bearer_with_conn()` (`api.rs` line 1436) performs an explicit subject GUID comparison:

```rust
if subject != expected_feed_guid {
    return Err(ApiError { status: StatusCode::FORBIDDEN, ... });
}
```

The `expected_feed_guid` comes from:
- For `PATCH /v1/feeds/{guid}`: the URL path parameter (`api.rs` line 1653)
- For `PATCH /v1/tracks/{guid}`: the track's `feed_guid` looked up from the DB (`api.rs` line 1710-1712)
- For `DELETE /v1/feeds/{guid}`: the URL path parameter (`api.rs` line 1111)
- For `DELETE /v1/feeds/{guid}/tracks/{track_guid}`: the URL path parameter (`api.rs` line 1246)

In all cases, the token's subject must exactly match the target feed.

**Evidence:** Tests `attack3_cross_feed_token_rejected` and `attack7b_track_patch_requires_parent_feed_token` both pass.

---

### 4. Admin Token Timing Attack

**Finding: PARTIALLY_PROTECTED**

**Scenario:** An attacker performs repeated requests with varying admin token values, measuring response times to determine the correct token byte-by-byte via a timing side-channel.

**Analysis:** The `check_admin_token()` function (`api.rs` line 965) uses standard string comparison:

```rust
if provided == expected {
    Ok(())
}
```

This is a non-constant-time comparison. Rust's `PartialEq` for strings compares byte-by-byte and short-circuits on the first mismatch. In theory, this leaks information about how many leading bytes of the token match.

**Practical exploitability:** Extremely low. The admin token is a high-entropy secret set at startup. Over a network, timing jitter (typically 0.1-10ms) vastly exceeds the sub-nanosecond signal from byte comparison differences. No known real-world exploitation of remote string comparison timing against high-entropy tokens exists.

**Note on bearer tokens:** The `validate_token()` function (`proof.rs` lines 155-156) uses an SQL `WHERE access_token = ?1` comparison, which is similarly non-constant-time. However, the code includes a security note (`proof.rs` lines 153-155) explaining why this is acceptable: "tokens are 128 bits of random entropy -- timing side-channels do not meaningfully reduce the search space for an attacker." This reasoning is sound.

**Evidence:** Test `attack4_admin_token_uses_non_constant_time_comparison` passes, documenting the behavior.

**Recommendation (low priority):** For maximum defense-in-depth, consider using `subtle::ConstantTimeEq` or `ring::constant_time::verify_slices_are_equal` for the admin token comparison. This is a belt-and-suspenders improvement, not a fix for a practical vulnerability.

---

### 5. Bearer Token Format Bypass

**Finding: PROTECTED**

**Sub-vectors tested:**

| Input | Result | Defense |
|-------|--------|---------|
| `Bearer ` (empty token) | Rejected (None) | `extract_bearer_token` checks `is_empty()` after trim (`api.rs` line 1387-1389) |
| `Bearer    ` (whitespace only) | Rejected (None) | `trim()` reduces to empty, caught by `is_empty()` |
| 1MB token string | No match (None) | `validate_token` SQL query returns no row; no crash |
| `'; DROP TABLE proof_tokens; --` | No match (None) | Parameterized queries (`params![]`) prevent SQL injection |
| `Basic dXNlcjpwYXNz` | Rejected (None) | `strip_prefix("Bearer ")` returns None for non-Bearer schemes |
| Raw token (no scheme) | Rejected (None) | `strip_prefix("Bearer ")` returns None |

**Note on null bytes:** HTTP header values cannot contain null bytes (Axum/hyper rejects them during parsing), so null byte injection is not possible at the token extraction layer.

**Evidence:** Tests `attack5a` through `attack5d` and `attack10` all pass.

---

### 6. Challenge Expiry Bypass

**Finding: PROTECTED**

**Scenario:** An attacker creates a challenge, waits for it to expire, then attempts to assert it.

**Analysis:** `get_challenge()` (`proof.rs` line 102) uses `WHERE expires_at > ?now` in the query:

```sql
SELECT ... FROM proof_challenges WHERE challenge_id = ?1 AND expires_at > ?2
```

The `now` value is computed fresh on each call (`proof.rs` line 98: `let now = now_secs()`). Expired challenges return `None`, which the handler translates to HTTP 404 (`api.rs` line 1563-1567).

There is no way to bypass this check -- the expiry is evaluated server-side using the server's clock, and expired rows simply do not appear in query results.

**Evidence:** Tests `attack6_expired_challenge_returns_none` and `attack6_expired_challenge_assert_returns_404` both pass.

---

### 7. Scope Confusion

**Finding: PROTECTED**

**Scenario:** A token with scope `feed:write` is used with a handler that requires a different scope, or the scope field is manipulated.

**Analysis:** Multiple layers of scope enforcement exist:

1. **Challenge creation** (`api.rs` line 1469-1475): Only `"feed:write"` is accepted as a scope. Any other scope returns HTTP 400.

2. **Token validation** (`proof.rs` line 169): The SQL query uses `WHERE scope = ?required_scope`, so a `feed:write` token cannot validate against `track:write` or any other scope.

3. **Handler consistency**: All mutation handlers (`handle_patch_feed`, `handle_patch_track`, `handle_retire_feed`, `handle_remove_track`) pass `"feed:write"` as the required scope to `check_admin_or_bearer_with_conn`.

4. **Track PATCH authorization** (`api.rs` lines 1700-1712): The handler looks up the track, retrieves its parent `feed_guid`, and validates the bearer token against that feed -- preventing cross-feed track modifications.

**Evidence:** Tests `attack7_scope_confusion_rejected` and `attack7b_track_patch_requires_parent_feed_token` both pass.

---

## Additional Findings

### Admin Token Header Priority

When both `X-Admin-Token` and `Authorization: Bearer` headers are present, the admin path takes priority (`api.rs` line 1417: `if headers.contains_key("X-Admin-Token")`). This is correct behavior -- admin should always be able to override. An attacker cannot bypass admin token validation by including a valid bearer token alongside a wrong admin token.

Evidence: Test `attack8_empty_admin_token_header_rejected` passes.

### TOCTOU Elimination

All authenticated handlers acquire the DB mutex before performing auth checks, holding the lock through both authentication and the subsequent database mutation. This eliminates TOCTOU (time-of-check/time-of-use) attacks where a token could be invalidated between auth check and write. This design is documented in code comments (e.g., `api.rs` line 1110: "Auth inside lock scope: eliminates TOCTOU between auth check and DB write").

### RFC 6750 Compliance

The `WWW-Authenticate` header is correctly emitted on all authentication failures:
- 401 (missing credentials): `Bearer realm="stophammer"`
- 401 (invalid token): `Bearer realm="stophammer", error="invalid_token"`
- 403 (wrong scope): `Bearer realm="stophammer", error="insufficient_scope"`

This is covered by the existing `rfc6750_tests.rs` test file (8 tests).

---

## Summary Table

| Attack Vector | Finding | Severity | Fix Required |
|--------------|---------|----------|-------------|
| 1. Token replay after feed deletion | PROTECTED | N/A | No |
| 2. Challenge race condition (double-spend) | PROTECTED | N/A | No |
| 3. Cross-feed token | PROTECTED | N/A | No |
| 4. Admin token timing attack | PARTIALLY_PROTECTED | Very Low | Optional |
| 5. Bearer token format bypass | PROTECTED | N/A | No |
| 6. Challenge expiry bypass | PROTECTED | N/A | No |
| 7. Scope confusion | PROTECTED | N/A | No |

**Overall assessment:** The authentication system is robust. No exploitable vulnerabilities were found. The one noted timing concern (admin token comparison) is a theoretical defense-in-depth issue with no practical exploit path.
