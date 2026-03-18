mod common;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use rand_core::OsRng;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

// ============================================================================
// Attack Vector 1: Signature Forgery
// ============================================================================

/// Verify that Ed25519 signature is bound to `event_type`.
/// An attacker who intercepts a signed `ArtistUpserted` event should NOT be able
/// to change it to `FeedUpserted` while keeping the same signature.
#[test]
fn signature_bound_to_event_type() {
    use stophammer::event::{EventSigningPayload, EventType};

    let key = SigningKey::generate(&mut OsRng);

    let payload_json = r#"{"artist":{"artist_id":"a1","name":"Test","name_lower":"test","created_at":1,"updated_at":1}}"#;

    // Sign as ArtistUpserted
    let payload_original = EventSigningPayload {
        event_id: "evt-1",
        event_type: &EventType::ArtistUpserted,
        payload_json,
        subject_guid: "subj-1",
        created_at: 9999,
        seq: 1, // Issue-SEQ-INTEGRITY — 2026-03-14
    };
    let serialized_original = serde_json::to_string(&payload_original).unwrap();
    let digest_original = Sha256::digest(serialized_original.as_bytes());
    let sig: Signature = key.sign(&digest_original);

    // Attempt to verify as FeedUpserted (different event_type)
    let payload_tampered = EventSigningPayload {
        event_id: "evt-1",
        event_type: &EventType::FeedUpserted,
        payload_json,
        subject_guid: "subj-1",
        created_at: 9999,
        seq: 1, // Issue-SEQ-INTEGRITY — 2026-03-14
    };
    let serialized_tampered = serde_json::to_string(&payload_tampered).unwrap();
    let digest_tampered = Sha256::digest(serialized_tampered.as_bytes());

    let verifier = key.verifying_key();
    let result = verifier.verify(&digest_tampered, &sig);
    assert!(
        result.is_err(),
        "VULNERABILITY: event_type is not covered by signature -- attacker can swap event types"
    );
}

/// Verify that the signing payload serialization is deterministic.
/// If `serde_json::to_string` produced different output for the same struct,
/// signature verification would break non-deterministically.
#[test]
fn signing_payload_serialization_is_deterministic() {
    use stophammer::event::{EventSigningPayload, EventType};

    let payload = EventSigningPayload {
        event_id: "evt-deterministic",
        event_type: &EventType::TrackUpserted,
        payload_json: r#"{"track_guid":"t1","title":"Test"}"#,
        subject_guid: "subj-1",
        created_at: 12345,
        seq: 1, // Issue-SEQ-INTEGRITY — 2026-03-14
    };

    let s1 = serde_json::to_string(&payload).unwrap();
    let s2 = serde_json::to_string(&payload).unwrap();
    let s3 = serde_json::to_string(&payload).unwrap();

    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
}

/// Verify that `EventSigningPayload` serialization preserves field declaration
/// order (not alphabetical). This matters because `serde_json::json!()` sorts
/// alphabetically, but `#[derive(Serialize)]` preserves declaration order.
#[test]
fn signing_payload_field_order_is_declaration_order() {
    use stophammer::event::{EventSigningPayload, EventType};

    let payload = EventSigningPayload {
        event_id: "evt-order",
        event_type: &EventType::ArtistUpserted,
        payload_json: "{}",
        subject_guid: "subj-1",
        created_at: 100,
        seq: 1, // Issue-SEQ-INTEGRITY — 2026-03-14
    };

    let serialized = serde_json::to_string(&payload).unwrap();

    // Declaration order: event_id, event_type, payload_json, subject_guid, created_at, seq
    // If alphabetical, it would be: created_at, event_id, event_type, payload_json, seq, subject_guid
    let event_id_pos = serialized.find("\"event_id\"").unwrap();
    let event_type_pos = serialized.find("\"event_type\"").unwrap();
    let payload_json_pos = serialized.find("\"payload_json\"").unwrap();
    let subject_guid_pos = serialized.find("\"subject_guid\"").unwrap();
    let created_at_pos = serialized.find("\"created_at\"").unwrap();
    let seq_pos = serialized.find("\"seq\"").unwrap();

    assert!(
        event_id_pos < event_type_pos,
        "event_id must come before event_type in serialized output"
    );
    assert!(
        event_type_pos < payload_json_pos,
        "event_type must come before payload_json"
    );
    assert!(
        payload_json_pos < subject_guid_pos,
        "payload_json must come before subject_guid"
    );
    assert!(
        subject_guid_pos < created_at_pos,
        "subject_guid must come before created_at"
    );
    assert!(created_at_pos < seq_pos, "created_at must come before seq");
}

/// Verify that `verify_event_signature` rejects an event with empty `payload_json`.
#[test]
fn verify_rejects_empty_payload_json() {
    use stophammer::event::{ArtistUpsertedPayload, Event, EventPayload, EventType};
    use stophammer::model::Artist;

    let artist = Artist {
        artist_id: "a1".into(),
        name: "Test".into(),
        name_lower: "test".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: 1,
        updated_at: 1,
    };

    let inner = ArtistUpsertedPayload { artist };
    let payload_json = serde_json::to_string(&inner).unwrap();

    let signer =
        stophammer::signing::NodeSigner::load_or_create("/tmp/crypto-sec-test.key").unwrap();
    let (signed_by, signature) = signer.sign_event(
        "evt-empty-pj",
        &EventType::ArtistUpserted,
        &payload_json,
        "subj-1",
        9999,
        1,
    );

    let event = Event {
        event_id: "evt-empty-pj".into(),
        event_type: EventType::ArtistUpserted,
        payload: EventPayload::ArtistUpserted(inner),
        payload_json: String::new(), // empty!
        subject_guid: "subj-1".into(),
        signed_by,
        signature,
        seq: 1,
        created_at: 9999,
        warnings: vec![],
    };

    let result = stophammer::signing::verify_event_signature(&event);
    assert!(
        result.is_err(),
        "verify_event_signature must reject events with empty payload_json"
    );
}

// ============================================================================
// Attack Vector 2: Nonce Reuse in Token Binding
// ============================================================================

/// Verify that reusing the same nonce across two challenges still produces
/// DIFFERENT `token_bindings` (because the server token is random each time).
#[test]
fn same_nonce_different_challenges_produce_different_bindings() {
    let conn = common::test_db();
    let nonce = "shared-nonce-value-16ch";

    let (_, binding1) =
        stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", nonce).unwrap();
    let (_, binding2) =
        stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", nonce).unwrap();

    assert_ne!(
        binding1, binding2,
        "same nonce must produce different bindings due to random server tokens"
    );
}

/// Verify that the nonce minimum length (16 chars) is enforced.
#[test]
fn nonce_minimum_length_enforced_at_api_layer() {
    // The enforcement is in the API handler (api.rs line 1477), not in proof.rs.
    // proof::create_challenge itself does NOT validate nonce length.
    // This test documents that proof::create_challenge accepts short nonces --
    // the validation is in the HTTP layer only.
    let conn = common::test_db();
    let result = stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", "short");
    assert!(
        result.is_ok(),
        "proof::create_challenge does not enforce nonce length -- API layer must do it"
    );
}

/// Verify that 128-bit server tokens have sufficient entropy.
/// Generate many tokens and check for collisions.
#[test]
fn server_tokens_have_sufficient_entropy() {
    let conn = common::test_db();
    let mut bindings = HashSet::new();

    for i in 0..100 {
        let nonce = format!("entropy-test-nonce-{i:04}");
        let (_, binding) =
            stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", &nonce).unwrap();

        // Extract just the token part (before the dot)
        let token_part = binding.split('.').next().unwrap().to_string();
        assert!(
            bindings.insert(token_part),
            "token collision detected after only {i} generations -- insufficient entropy"
        );
    }
}

// ============================================================================
// Attack Vector 3: Token Binding Malleability
// ============================================================================

/// Verify that base64url-encoded tokens never contain '.', so `split_once('.')`
/// always correctly separates token from hash.
#[test]
fn base64url_tokens_never_contain_dot() {
    let conn = common::test_db();

    for i in 0..50 {
        let nonce = format!("dot-test-nonce-number-{i:04}");
        let (_, binding) =
            stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", &nonce).unwrap();

        let parts: Vec<&str> = binding.split('.').collect();
        assert_eq!(
            parts.len(),
            2,
            "binding should have exactly one dot separator, got {} parts for binding: {}",
            parts.len(),
            binding
        );

        // Verify each part is valid base64url
        assert!(
            URL_SAFE_NO_PAD.decode(parts[0]).is_ok(),
            "token part should be valid base64url"
        );
        assert!(
            URL_SAFE_NO_PAD.decode(parts[1]).is_ok(),
            "hash part should be valid base64url"
        );
    }
}

/// Verify that `recompute_binding` correctly handles a binding where the
/// `base_token` part could be confused with a multi-dot string.
/// Since base64url never contains dots, this is a defense-in-depth check.
#[test]
fn recompute_binding_multi_dot_takes_first_only() {
    // If somehow the token contained a dot (it shouldn't with base64url),
    // split_once would take everything after the first dot as hash_part.
    let result = stophammer::proof::recompute_binding("part1.part2.part3", "nonce-16-chars-ok");
    // split_once('.') on "part1.part2.part3" gives ("part1", "part2.part3")
    // base_token = "part1", hash_part = "part2.part3" (both non-empty)
    // So it returns Some(...) with base_token = "part1"
    assert!(
        result.is_some(),
        "multi-dot input should not return None (split_once takes first dot)"
    );
    let binding = result.unwrap();
    assert!(
        binding.starts_with("part1."),
        "recomputed binding should use 'part1' as the base token"
    );
}

// ============================================================================
// Attack Vector 4: Event ID Collision
// ============================================================================

/// Verify that `UUIDv4` event IDs have sufficient entropy (no collisions in batch).
#[test]
fn event_ids_are_unique_uuid_v4() {
    let mut ids = HashSet::new();
    for _ in 0..1000 {
        let id = uuid::Uuid::new_v4().to_string();
        assert!(
            ids.insert(id),
            "UUID v4 collision detected -- this should be astronomically unlikely"
        );
    }
}

/// Verify that the events table has a PRIMARY KEY constraint on `event_id`,
/// preventing duplicate `event_ids`.
#[test]
fn event_id_primary_key_prevents_duplicates() {
    let conn = common::test_db();
    let now = common::now();

    conn.execute(
        "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
         VALUES ('dup-evt', 'artist_upserted', '{}', 'subj', 'pk', 'sig', 1, ?1, '[]')",
        params![now],
    ).unwrap();

    let result = conn.execute(
        "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, signature, seq, created_at, warnings_json) \
         VALUES ('dup-evt', 'artist_upserted', '{}', 'subj', 'pk', 'sig', 2, ?1, '[]')",
        params![now],
    );

    assert!(
        result.is_err(),
        "duplicate event_id must be rejected by PRIMARY KEY constraint"
    );
}

// ============================================================================
// Attack Vector 5: Signed Payload Injection (event_type swap)
// ============================================================================

/// Verify that changing ANY field in the signing payload breaks the signature.
/// This is the comprehensive version covering all fields.
#[test]
fn signature_covers_all_payload_fields() {
    use stophammer::event::{ArtistUpsertedPayload, Event, EventPayload, EventType};
    use stophammer::model::Artist;

    let signer =
        stophammer::signing::NodeSigner::load_or_create("/tmp/crypto-sec-test-2.key").unwrap();

    let artist = Artist {
        artist_id: "a1".into(),
        name: "Test".into(),
        name_lower: "test".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: 1,
        updated_at: 1,
    };

    let inner = ArtistUpsertedPayload { artist };
    let payload_json = serde_json::to_string(&inner).unwrap();

    let (signed_by, signature) = signer.sign_event(
        "evt-fields",
        &EventType::ArtistUpserted,
        &payload_json,
        "subj-1",
        9999,
        1,
    );

    // Helper: build event with given overrides
    let make_event = |event_id: &str, event_type: EventType, pj: &str, sg: &str, ca: i64| Event {
        event_id: event_id.into(),
        event_type,
        payload: EventPayload::ArtistUpserted(inner.clone()),
        payload_json: pj.into(),
        subject_guid: sg.into(),
        signed_by: signed_by.clone(),
        signature: signature.clone(),
        seq: 1,
        created_at: ca,
        warnings: vec![],
    };

    // Original should verify
    let original = make_event(
        "evt-fields",
        EventType::ArtistUpserted,
        &payload_json,
        "subj-1",
        9999,
    );
    assert!(
        stophammer::signing::verify_event_signature(&original).is_ok(),
        "original event should verify"
    );

    // Tamper event_id
    let tampered_id = make_event(
        "evt-TAMPERED",
        EventType::ArtistUpserted,
        &payload_json,
        "subj-1",
        9999,
    );
    assert!(
        stophammer::signing::verify_event_signature(&tampered_id).is_err(),
        "tampered event_id must break signature"
    );

    // Tamper event_type
    let tampered_type = make_event(
        "evt-fields",
        EventType::FeedUpserted,
        &payload_json,
        "subj-1",
        9999,
    );
    assert!(
        stophammer::signing::verify_event_signature(&tampered_type).is_err(),
        "tampered event_type must break signature"
    );

    // Tamper payload_json
    let tampered_payload = make_event(
        "evt-fields",
        EventType::ArtistUpserted,
        r#"{"tampered":true}"#,
        "subj-1",
        9999,
    );
    assert!(
        stophammer::signing::verify_event_signature(&tampered_payload).is_err(),
        "tampered payload_json must break signature"
    );

    // Tamper subject_guid
    let tampered_guid = make_event(
        "evt-fields",
        EventType::ArtistUpserted,
        &payload_json,
        "TAMPERED",
        9999,
    );
    assert!(
        stophammer::signing::verify_event_signature(&tampered_guid).is_err(),
        "tampered subject_guid must break signature"
    );

    // Tamper created_at
    let tampered_ts = make_event(
        "evt-fields",
        EventType::ArtistUpserted,
        &payload_json,
        "subj-1",
        0,
    );
    assert!(
        stophammer::signing::verify_event_signature(&tampered_ts).is_err(),
        "tampered created_at must break signature"
    );
}

// ============================================================================
// Attack Vector 6: Content Hash Collision
// ============================================================================

/// Verify that `content_hash` is used only for deduplication, not for security.
/// The hash determines whether a crawl is a no-op (`NO_CHANGE`), but is not
/// included in the event signature.
#[test]
fn content_hash_not_in_signing_payload() {
    use stophammer::event::{EventSigningPayload, EventType};

    let payload = EventSigningPayload {
        event_id: "evt-hash",
        event_type: &EventType::FeedUpserted,
        payload_json: "{}",
        subject_guid: "subj-1",
        created_at: 100,
        seq: 1, // Issue-SEQ-INTEGRITY — 2026-03-14
    };
    let serialized = serde_json::to_string(&payload).unwrap();

    // content_hash should not appear in the signing payload
    assert!(
        !serialized.contains("content_hash"),
        "content_hash should not be part of EventSigningPayload -- it is not security-sensitive"
    );
}

// ============================================================================
// Attack Vector 7: Base64 Decoding Attacks
// ============================================================================

/// Verify that the nonce is used as raw bytes, not base64-decoded.
/// An attacker sending non-base64 nonce should not crash the server.
#[test]
fn nonce_with_non_base64_chars_does_not_crash() {
    let conn = common::test_db();

    // Various adversarial nonces
    let adversarial_nonces = [
        "!!!@@@###$$$%%%%",
        "\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f",
        "unicode-test-\u{1F600}\u{1F600}\u{1F600}",
        "a]b[c}d{e|f\\g/h",
        &"A".repeat(10_000), // very long nonce
    ];

    for nonce in &adversarial_nonces {
        let result = stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", nonce);
        assert!(
            result.is_ok(),
            "create_challenge should not crash on adversarial nonce"
        );
    }
}

/// Verify that `recompute_binding` handles adversarial `stored_binding` inputs.
#[test]
fn recompute_binding_adversarial_inputs() {
    // Various malformed stored_binding strings
    let cases = [
        ("", false),       // empty string -- no dot
        (".", false),      // just a dot -- both parts empty
        (".hash", false),  // empty base_token
        ("token.", false), // empty hash_part
        ("a.b", true),     // minimal valid
        ("a.b.c.d", true), // extra dots -- split_once takes first
    ];

    for (input, should_be_some) in &cases {
        let result = stophammer::proof::recompute_binding(input, "test-nonce-16-chars");
        assert_eq!(
            result.is_some(),
            *should_be_some,
            "recompute_binding({input:?}) returned {result:?}, expected is_some={should_be_some}",
        );
    }
}

// ============================================================================
// Attack Vector 8: Proof-of-Possession Bypass (Missing RSS Verification)
// ============================================================================

/// VULNERABILITY PROOF: The proof-of-possession flow issues tokens WITHOUT
/// verifying that the requester actually controls the feed via `podcast:txt`.
///
/// An attacker who knows a `feed_guid` can obtain a write token for that feed
/// by simply completing the challenge-assert flow with any nonce.
///
/// Evidence: `api.rs` line 1593:
///   `// TODO: fetch RSS at feed_url and verify podcast:txt token before issuing -- Phase 2`
#[test]
fn proof_of_possession_issues_token_without_feed_verification() {
    let conn = common::test_db();

    // Attacker creates a challenge for a feed they don't own
    let attacker_nonce = "attacker-nonce-1234";
    let (challenge_id, token_binding) = stophammer::proof::create_challenge(
        &conn,
        "victim-feed-guid",
        "feed:write",
        attacker_nonce,
    )
    .unwrap();

    // Attacker can recompute the binding (they know the nonce, the binding was returned)
    let recomputed = stophammer::proof::recompute_binding(&token_binding, attacker_nonce);
    assert_eq!(
        recomputed.as_deref(),
        Some(token_binding.as_str()),
        "attacker can always produce matching binding since they chose the nonce"
    );

    // In the real flow, the assert handler would issue a token here.
    // The challenge is valid and the nonce matches -- there is no RSS verification.
    stophammer::proof::resolve_challenge(&conn, &challenge_id, "valid").unwrap();
    let access_token = stophammer::proof::issue_token(
        &conn,
        "feed:write",
        "victim-feed-guid",
        &stophammer::proof::ProofLevel::RssOnly,
    )
    .unwrap();

    // Attacker now has a valid token for the victim's feed
    let subject = stophammer::proof::validate_token(&conn, &access_token, "feed:write").unwrap();
    assert_eq!(
        subject,
        Some("victim-feed-guid".to_string()),
        "VULNERABILITY: attacker obtained write token for victim feed without proving ownership"
    );
}

// ============================================================================
// Attack Vector 9: Replay of Resolved Challenge
// ============================================================================

/// Verify that a challenge cannot be asserted twice (replay protection).
#[test]
fn resolved_challenge_cannot_be_replayed() {
    let conn = common::test_db();
    let nonce = "replay-test-nonce-ok";

    let (challenge_id, _) =
        stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", nonce).unwrap();

    // First resolution succeeds
    stophammer::proof::resolve_challenge(&conn, &challenge_id, "valid").unwrap();

    // Verify it's now "valid"
    let ch = stophammer::proof::get_challenge(&conn, &challenge_id)
        .unwrap()
        .unwrap();
    assert_eq!(ch.state, "valid");

    // Second resolve_challenge is a no-op (WHERE state = 'pending' won't match)
    stophammer::proof::resolve_challenge(&conn, &challenge_id, "valid").unwrap();

    // The state doesn't change and no error is thrown -- this is correct behavior
    let ch2 = stophammer::proof::get_challenge(&conn, &challenge_id)
        .unwrap()
        .unwrap();
    assert_eq!(ch2.state, "valid");
}

// ============================================================================
// Attack Vector 10: Community Node Signature Verification
// ============================================================================

/// Verify that community nodes reject events signed by unknown keys.
/// This is tested at the data model level since we can't easily spin up
/// a full community node in unit tests.
#[test]
fn verify_event_signature_rejects_unknown_signer() {
    use stophammer::event::{ArtistUpsertedPayload, Event, EventPayload, EventType};
    use stophammer::model::Artist;

    let signer =
        stophammer::signing::NodeSigner::load_or_create("/tmp/crypto-sec-test-3.key").unwrap();
    let attacker_key = SigningKey::generate(&mut OsRng);
    let attacker_pubkey = hex::encode(attacker_key.verifying_key().to_bytes());

    let artist = Artist {
        artist_id: "a1".into(),
        name: "Test".into(),
        name_lower: "test".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: 1,
        updated_at: 1,
    };

    let inner = ArtistUpsertedPayload { artist };
    let payload_json = serde_json::to_string(&inner).unwrap();

    let (_, signature) = signer.sign_event(
        "evt-unknown",
        &EventType::ArtistUpserted,
        &payload_json,
        "subj-1",
        9999,
        1,
    );

    // Build event with attacker's pubkey but legitimate node's signature
    let event = Event {
        event_id: "evt-unknown".into(),
        event_type: EventType::ArtistUpserted,
        payload: EventPayload::ArtistUpserted(inner),
        payload_json,
        subject_guid: "subj-1".into(),
        signed_by: attacker_pubkey, // wrong key
        signature,                  // signature from different key
        seq: 1,
        created_at: 9999,
        warnings: vec![],
    };

    let result = stophammer::signing::verify_event_signature(&event);
    assert!(
        result.is_err(),
        "signature verification must fail when signed_by doesn't match the actual signer"
    );
}

// ============================================================================
// v2 Attack Surfaces
// ============================================================================

// ── N1: SipHash FTS5 Rowid Collisions ──────────────────────────────────────

/// Verify that `SipHash` rowid computation is deterministic and stable.
/// The same (`entity_type`, `entity_id`) must always produce the same rowid.
#[test]
fn siphash_rowid_is_deterministic() {
    let r1 = stophammer::search::rowid_for("feed", "abc-123");
    let r2 = stophammer::search::rowid_for("feed", "abc-123");
    let r3 = stophammer::search::rowid_for("feed", "abc-123");
    assert_eq!(r1, r2);
    assert_eq!(r2, r3);
}

/// Verify that `SipHash` rowids are always positive (63-bit masking).
#[test]
fn siphash_rowid_is_always_positive() {
    let test_cases = [
        ("feed", "guid-1"),
        ("track", "guid-2"),
        ("artist", "some-long-artist-id-with-many-characters"),
        ("feed", ""),
        ("", "guid"),
    ];
    for (et, eid) in &test_cases {
        let rowid = stophammer::search::rowid_for(et, eid);
        assert!(
            rowid >= 0,
            "rowid must be non-negative, got {rowid} for ({et}, {eid})"
        );
    }
}

/// Verify that the NUL separator prevents prefix collisions.
/// ("ab", "c") and ("a", "bc") must produce different rowids.
#[test]
fn siphash_nul_separator_prevents_prefix_collision() {
    let r1 = stophammer::search::rowid_for("ab", "c");
    let r2 = stophammer::search::rowid_for("a", "bc");
    assert_ne!(
        r1, r2,
        "NUL separator must distinguish ('ab','c') from ('a','bc')"
    );
}

/// Verify that FTS5 contentless index handles rowid collision gracefully:
/// if two entities collide, the second overwrites the first in the index.
/// This is a data-quality issue, not a security breach.
#[test]
fn fts5_rowid_collision_overwrites_not_crashes() {
    let conn = common::test_db();

    // Insert entity A
    stophammer::search::populate_search_index(
        &conn, "feed", "entity-a", "Name A", "Title A", "Desc A", "tag-a",
    )
    .expect("first insert should succeed");

    // Insert entity B with possibly different rowid (we just verify no crash)
    stophammer::search::populate_search_index(
        &conn, "feed", "entity-b", "Name B", "Title B", "Desc B", "tag-b",
    )
    .expect("second insert should succeed regardless of rowid collision");
}

// ── N2: Constant-Time Admin Token Comparison ───────────────────────────────

/// Verify that SHA-256(x) == SHA-256(y) iff x == y (no false positives).
/// This confirms the `check_admin_token` approach is functionally correct.
#[test]
fn sha256_equality_implies_input_equality() {
    let tokens = ["secret-token-1", "secret-token-2", "secret-token-1"];
    let h0 = sha2::Sha256::digest(tokens[0].as_bytes());
    let h1 = sha2::Sha256::digest(tokens[1].as_bytes());
    let h2 = sha2::Sha256::digest(tokens[2].as_bytes());

    // Same input -> same hash
    assert_eq!(h0, h2, "identical tokens must produce identical hashes");
    // Different input -> different hash
    assert_ne!(h0, h1, "different tokens must produce different hashes");
}

// ── N3: Token Binding Cross-Challenge Replay ───────────────────────────────

/// Verify that knowing a nonce from challenge A does NOT help resolve challenge B.
/// Each challenge has a unique `base_token`, so the binding is different even with
/// the same nonce.
#[test]
fn cross_challenge_nonce_replay_fails() {
    let conn = common::test_db();
    let shared_nonce = "shared-nonce-for-replay-test";

    // Create two challenges with the same nonce
    let (id_a, binding_a) =
        stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", shared_nonce).unwrap();
    let (_id_b, binding_b) =
        stophammer::proof::create_challenge(&conn, "feed-1", "feed:write", shared_nonce).unwrap();

    // The bindings are different (different base_tokens)
    assert_ne!(binding_a, binding_b);

    // Recomputing binding_b with the shared nonce produces binding_b, not binding_a
    let recomputed_b = stophammer::proof::recompute_binding(&binding_b, shared_nonce).unwrap();
    assert_eq!(recomputed_b, binding_b);
    assert_ne!(recomputed_b, binding_a);

    // Attempting to use nonce from challenge B to validate challenge A fails
    // because recompute_binding(binding_a, nonce) == binding_a (it works for A),
    // but you cannot use knowledge of B to forge A's binding
    let recomputed_a = stophammer::proof::recompute_binding(&binding_a, shared_nonce).unwrap();
    assert_eq!(recomputed_a, binding_a, "nonce validates its own challenge");

    // A wrong nonce will NOT recompute correctly
    let wrong_nonce = "totally-different-nonce!!";
    let wrong_recompute = stophammer::proof::recompute_binding(&binding_a, wrong_nonce).unwrap();
    assert_ne!(
        wrong_recompute, binding_a,
        "wrong nonce must NOT produce a matching binding"
    );

    // Resolve challenge A to prove single-use
    let rows = stophammer::proof::resolve_challenge(&conn, &id_a, "valid").unwrap();
    assert_eq!(rows, 1, "first resolution should affect 1 row");

    // Second resolution is a no-op
    let rows2 = stophammer::proof::resolve_challenge(&conn, &id_a, "valid").unwrap();
    assert_eq!(rows2, 0, "second resolution should be a no-op");
}

// ── N5: SSRF Validation ────────────────────────────────────────────────────

/// Verify that `validate_feed_url` rejects private/reserved IP addresses.
#[test]
fn ssrf_rejects_private_ips() {
    let rejected = [
        "http://127.0.0.1/feed.xml",
        "http://10.0.0.1/feed.xml",
        "http://172.16.0.1/feed.xml",
        "http://192.168.1.1/feed.xml",
        "http://169.254.0.1/feed.xml",
        "http://0.0.0.0/feed.xml",
        "http://[::1]/feed.xml",
        "http://[::]/feed.xml",
        "http://[fc00::1]/feed.xml",
        "http://[fe80::1]/feed.xml",
    ];

    for url in &rejected {
        let result = stophammer::proof::validate_feed_url(url);
        assert!(
            result.is_err(),
            "validate_feed_url should reject private IP URL: {url}"
        );
    }
}

/// Verify that `validate_feed_url` rejects non-HTTP schemes.
#[test]
fn ssrf_rejects_non_http_schemes() {
    let rejected = [
        "file:///etc/passwd",
        "ftp://example.com/feed.xml",
        "gopher://example.com/feed.xml",
        "data:text/xml,<rss/>",
    ];

    for url in &rejected {
        let result = stophammer::proof::validate_feed_url(url);
        assert!(
            result.is_err(),
            "validate_feed_url should reject non-HTTP URL: {url}"
        );
    }
}

/// Verify that `validate_feed_url` accepts legitimate public HTTP(S) URLs.
#[test]
fn ssrf_accepts_public_urls() {
    // These should pass the scheme + IP checks (DNS resolution may fail
    // in test environments, but the function allows resolution failures
    // to pass through to the HTTP client).
    let accepted = [
        "https://feeds.example.com/podcast.xml",
        "http://feeds.example.com/podcast.xml",
    ];

    for url in &accepted {
        let result = stophammer::proof::validate_feed_url(url);
        // DNS resolution might fail in test env, but scheme + literal IP checks pass
        assert!(
            result.is_ok(),
            "validate_feed_url should accept public URL: {url}, got: {:?}",
            result.err()
        );
    }
}

/// Verify that `validate_feed_url` rejects CGNAT range (100.64.0.0/10).
#[test]
fn ssrf_rejects_cgnat_range() {
    let result = stophammer::proof::validate_feed_url("http://100.64.0.1/feed.xml");
    assert!(
        result.is_err(),
        "validate_feed_url should reject CGNAT IP 100.64.0.1"
    );

    // 100.127.255.254 is also in 100.64.0.0/10
    let result2 = stophammer::proof::validate_feed_url("http://100.127.255.254/feed.xml");
    assert!(
        result2.is_err(),
        "validate_feed_url should reject CGNAT IP 100.127.255.254"
    );
}

/// Verify that `validate_feed_url` handles edge-case URLs.
/// `http:///feed.xml` parses as having an empty host in the `url` crate.
/// The empty host cannot resolve via DNS, so the validator must now reject it
/// instead of silently deferring to the HTTP client.
#[test]
fn ssrf_empty_host_rejected() {
    let result = stophammer::proof::validate_feed_url("http:///feed.xml");
    assert!(
        result.is_err(),
        "empty-host feed URLs must be rejected instead of falling back to runtime DNS"
    );
}

/// Verify that `validate_feed_url` rejects truly malicious non-URL strings.
#[test]
fn ssrf_rejects_garbage_input() {
    let result = stophammer::proof::validate_feed_url("not-a-url");
    assert!(
        result.is_err(),
        "validate_feed_url should reject unparseable input"
    );
}
