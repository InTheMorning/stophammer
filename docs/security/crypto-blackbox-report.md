# Cryptographic Security Black-Box Report

**Date:** 2026-03-12
**Scope:** Signature verification, token binding, nonce handling, event IDs, payload canonicalization
**Files audited:**
- `src/signing.rs` -- Ed25519 key management and event signing/verification
- `src/proof.rs` -- ACME-inspired challenge/assert proof-of-possession flow
- `src/api.rs` -- HTTP handlers for `/v1/proofs/challenge`, `/v1/proofs/assert`, bearer auth
- `src/event.rs` -- `Event`, `EventSigningPayload`, `EventPayload` types
- `src/community.rs` -- Community node push receiver and signature filtering
- `src/apply.rs` -- Event application with signature verification
- `Cargo.toml` -- Dependency versions

**Test file:** `tests/crypto_security_tests.rs` (18 tests, all passing)

---

## Executive Summary

The stophammer cryptographic layer is well-designed. Ed25519 signatures correctly cover all security-relevant fields (event_id, event_type, payload_json, subject_guid, created_at). The signing payload serialization is deterministic and uses declaration-order field serialization. Token binding uses 128-bit entropy with CSPRNG. Base64url encoding eliminates dot-separator ambiguity in `recompute_binding`.

One **critical design gap** was identified: the proof-of-possession flow (`/v1/proofs/challenge` and `/v1/proofs/assert`) issues write tokens without verifying that the requester controls the feed via `podcast:txt` DNS/RSS records. This is acknowledged in the codebase as a Phase 2 TODO (api.rs line 1593). Until implemented, any party who knows a `feed_guid` can obtain a `feed:write` token.

One **low-severity observation** was noted: the `requester_nonce` is not checked for uniqueness across challenges at the server side. This is acceptable because each challenge generates a fresh 128-bit server token, making the token_binding unique regardless of nonce reuse.

---

## Attack Vectors

### 1. Signature Forgery

**Finding: PROTECTED**

- **Curve:** Ed25519 via `ed25519-dalek` v2 with `rand_core` feature.
- **Key generation:** `SigningKey::generate(&mut OsRng)` -- CSPRNG from OS.
- **Signing process:** SHA-256 digest of canonical JSON serialization of `EventSigningPayload`, then Ed25519 sign over the 32-byte digest.
- **Verification:** Reconstructs the same `EventSigningPayload` from the `Event` struct and verifies.
- **Empty payload guard:** `verify_event_signature` explicitly rejects events with empty `payload_json` (signing.rs line 169-174).

Ed25519 is a well-studied curve with no known practical forgery attacks. The implementation uses the standard `ed25519-dalek` crate which is widely audited.

**Evidence:** Test `signature_covers_all_payload_fields` proves that tampering any single field (event_id, event_type, payload_json, subject_guid, created_at) breaks the signature. Test `verify_event_signature_rejects_unknown_signer` proves mismatched pubkey is rejected.

### 2. Nonce Reuse in Token Binding

**Finding: PROTECTED**

- The `requester_nonce` is NOT checked for uniqueness server-side.
- However, each challenge generates a fresh 128-bit random server token via `OsRng.fill_bytes(&mut [0u8; 16])`.
- The `token_binding = server_token || '.' || base64url(SHA-256(requester_nonce))`.
- Even if an attacker reuses the same nonce, each challenge produces a different binding due to the random server token.
- Minimum nonce length of 16 characters is enforced at the API layer (api.rs lines 1477, 1543).

**Evidence:** Test `same_nonce_different_challenges_produce_different_bindings` proves this. Test `server_tokens_have_sufficient_entropy` generates 100 tokens with no collisions.

**Note:** `proof::create_challenge` itself does not enforce the 16-character minimum. The validation occurs in the HTTP handler layer. Test `nonce_minimum_length_enforced_at_api_layer` documents this.

### 3. Token Binding Malleability

**Finding: PROTECTED**

- The binding format is `base64url_token.base64url_hash`.
- Base64url alphabet (`A-Za-z0-9_-`) never contains `.`.
- `split_once('.')` in `recompute_binding` correctly separates the token from the hash portion.
- Empty-part guards reject bindings with empty token or empty hash (proof.rs lines 187-189).
- Multi-dot inputs are handled safely: `split_once` takes the first dot, so `a.b.c` splits into `("a", "b.c")`.

**Evidence:** Test `base64url_tokens_never_contain_dot` generates 50 bindings and verifies each contains exactly one dot with valid base64url on both sides. Test `recompute_binding_adversarial_inputs` covers edge cases including empty string, lone dot, empty parts, and multi-dot strings.

### 4. Event ID Collision

**Finding: PROTECTED**

- Event IDs are generated via `uuid::Uuid::new_v4().to_string()` -- 122 bits of entropy from CSPRNG.
- The `events` table has `event_id TEXT PRIMARY KEY`, preventing any duplicate insertion.
- Community nodes use `INSERT OR IGNORE` for idempotent replay (db.rs `insert_event_idempotent`).

**Evidence:** Test `event_ids_are_unique_uuid_v4` generates 1000 UUIDs with no collisions. Test `event_id_primary_key_prevents_duplicates` proves the DB rejects duplicate event_ids.

### 5. Signed Payload Injection (Event Type Swap)

**Finding: PROTECTED**

- `event_type` is included in `EventSigningPayload` (event.rs line 107).
- Changing the event_type invalidates the signature.
- `payload_json` is serialized once at sign time and stored as a string field in the signing payload, avoiding re-serialization ambiguity.
- `EventSigningPayload` uses `#[derive(Serialize)]` which preserves **declaration order**, not alphabetical order. This is deterministic across invocations.

**Evidence:** Test `signature_bound_to_event_type` proves swapping `ArtistUpserted` to `FeedUpserted` breaks the signature. Test `signing_payload_field_order_is_declaration_order` verifies fields appear in declaration order. Test `signing_payload_serialization_is_deterministic` proves triple serialization produces identical output.

### 6. Content Hash Collision

**Finding: PROTECTED (not security-relevant)**

- `content_hash` is SHA-256 of the feed body.
- It is used exclusively for deduplication in `ContentHashVerifier` (verifiers/content_hash.rs).
- It is NOT included in the event signature payload.
- A SHA-256 collision would only cause a changed feed to be treated as unchanged (a no-op), not a security breach.
- Finding a SHA-256 collision requires ~2^128 work, which is infeasible.

**Evidence:** Test `content_hash_not_in_signing_payload` verifies content_hash is absent from `EventSigningPayload` serialization.

### 7. Base64 Decoding Attacks

**Finding: PROTECTED**

- The `requester_nonce` is used as raw bytes via `.as_bytes()` (proof.rs line 53).
- No base64 decoding is performed on the nonce -- it is fed directly into SHA-256.
- Adversarial inputs (null bytes, unicode, very long strings, special characters) do not crash the system.

**Evidence:** Test `nonce_with_non_base64_chars_does_not_crash` sends various adversarial nonces including null bytes, unicode emoji, special characters, and a 10,000-character string. All succeed without panic.

### 8. Proof-of-Possession Bypass (Missing RSS Verification)

**Finding: VULNERABLE**

**Severity: Critical (design gap, acknowledged as Phase 2)**

The proof-of-possession flow issues `feed:write` tokens without verifying that the requester actually controls the feed. The intended verification step -- fetching the feed's RSS and checking for a `podcast:txt` record containing the token_binding -- is not yet implemented.

**Code reference:** `src/api.rs` line 1593:
```
// TODO: fetch RSS at feed_url and verify podcast:txt token before issuing -- Phase 2
```

**Attack scenario:**
1. Attacker sends `POST /v1/proofs/challenge` with `feed_guid` = victim's feed GUID and any nonce.
2. Server returns `challenge_id` and `token_binding`.
3. Attacker sends `POST /v1/proofs/assert` with the `challenge_id` and the same nonce.
4. Server issues an `access_token` scoped to `feed:write` for the victim's feed.
5. Attacker uses the token to `PATCH /v1/feeds/{guid}` or `PATCH /v1/tracks/{guid}`.

**Mitigation:** The `handle_proofs_assert` handler must, before marking the challenge as valid, fetch the feed's RSS and verify that the `token_binding` appears in a `podcast:txt` tag. Until then, the admin token (`X-Admin-Token`) is the only safe authorization mechanism for mutations.

**Evidence:** Test `proof_of_possession_issues_token_without_feed_verification` proves end-to-end that an attacker can obtain a write token for any feed_guid without proving ownership.

### 9. Challenge Replay

**Finding: PROTECTED**

- Challenges are single-use: `resolve_challenge` uses `WHERE state = 'pending'`, so already-resolved challenges cannot be re-resolved.
- The assert handler checks `challenge.state != "pending"` and returns 400 if already resolved (api.rs line 1570).
- Challenges expire after 24 hours (proof.rs line 58).

**Evidence:** Test `resolved_challenge_cannot_be_replayed` proves that a second `resolve_challenge` call is a no-op.

### 10. Community Node Event Filtering

**Finding: PROTECTED**

- The push handler (`handle_sync_push` in community.rs) filters incoming events: only events where `ev.signed_by == state.primary_pubkey_hex` are accepted (community.rs lines 319-330).
- Events from unknown signers are rejected before signature verification.
- `apply::apply_events` then verifies the ed25519 signature on each accepted event (apply.rs line 235).
- This is defense-in-depth: pubkey filtering + cryptographic signature verification.

**Evidence:** Test `verify_event_signature_rejects_unknown_signer` proves that an event with a mismatched pubkey fails verification.

---

## Summary Table

| # | Attack Vector | Finding | Severity |
|---|---|---|---|
| 1 | Signature Forgery | PROTECTED | N/A |
| 2 | Nonce Reuse | PROTECTED | N/A |
| 3 | Token Binding Malleability | PROTECTED | N/A |
| 4 | Event ID Collision | PROTECTED | N/A |
| 5 | Signed Payload Injection | PROTECTED | N/A |
| 6 | Content Hash Collision | PROTECTED | N/A |
| 7 | Base64 Decoding Attacks | PROTECTED | N/A |
| 8 | Proof-of-Possession Bypass | VULNERABLE | Critical |
| 9 | Challenge Replay | PROTECTED | N/A |
| 10 | Community Node Filtering | PROTECTED | N/A |

---

## Recommendations

1. **Implement `podcast:txt` RSS verification (Phase 2 TODO)** -- This is the only critical finding. The assert handler must fetch the feed RSS and verify the token_binding before issuing an access token.

2. **Consider rate-limiting `/v1/proofs/challenge`** -- Without RSS verification, an attacker can create unlimited challenges. Even after RSS verification is implemented, rate-limiting prevents abuse of the challenge creation endpoint.

3. **Consider nonce uniqueness tracking** -- While not strictly necessary (server tokens provide uniqueness), logging or deduplicating nonces could detect automated abuse patterns.
