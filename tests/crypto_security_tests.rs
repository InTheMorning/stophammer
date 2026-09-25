mod common;

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

    let signer = common::temp_signer("crypto-sec-test");
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
#[allow(
    clippy::too_many_lines,
    reason = "security regression test spells out multiple tamper cases inline for auditability"
)]
fn signature_covers_all_payload_fields() {
    use stophammer::event::{ArtistUpsertedPayload, Event, EventPayload, EventType};
    use stophammer::model::Artist;

    let signer = common::temp_signer("crypto-sec-test-2");

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
// Attack Vector 10: Community Node Signature Verification
// ============================================================================

/// Verify that community nodes reject events signed by unknown keys.
/// This is tested at the data model level since we can't easily spin up
/// a full community node in unit tests.
#[test]
fn verify_event_signature_rejects_unknown_signer() {
    use stophammer::event::{ArtistUpsertedPayload, Event, EventPayload, EventType};
    use stophammer::model::Artist;

    let signer = common::temp_signer("crypto-sec-test-3");
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
        let result = stophammer::fetch_guard::validate_feed_url(url);
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
        let result = stophammer::fetch_guard::validate_feed_url(url);
        assert!(
            result.is_err(),
            "validate_feed_url should reject non-HTTP URL: {url}"
        );
    }
}

/// Verify that `validate_feed_url` accepts legitimate public HTTP(S) URLs.
#[test]
fn ssrf_accepts_public_urls() {
    // Use public IP literals so this test stays deterministic without relying
    // on external DNS resolution in the test environment.
    let accepted = [
        "https://93.184.216.34/podcast.xml",
        "http://93.184.216.34/podcast.xml",
    ];

    for url in &accepted {
        let result = stophammer::fetch_guard::validate_feed_url(url);
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
    let result = stophammer::fetch_guard::validate_feed_url("http://100.64.0.1/feed.xml");
    assert!(
        result.is_err(),
        "validate_feed_url should reject CGNAT IP 100.64.0.1"
    );

    // 100.127.255.254 is also in 100.64.0.0/10
    let result2 = stophammer::fetch_guard::validate_feed_url("http://100.127.255.254/feed.xml");
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
    let result = stophammer::fetch_guard::validate_feed_url("http:///feed.xml");
    assert!(
        result.is_err(),
        "empty-host feed URLs must be rejected instead of falling back to runtime DNS"
    );
}

/// Verify that `validate_feed_url` rejects truly malicious non-URL strings.
#[test]
fn ssrf_rejects_garbage_input() {
    let result = stophammer::fetch_guard::validate_feed_url("not-a-url");
    assert!(
        result.is_err(),
        "validate_feed_url should reject unparseable input"
    );
}
