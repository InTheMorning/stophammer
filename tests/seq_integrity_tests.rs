mod common;

use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Issue-SEQ-INTEGRITY: inflated ev.seq must not advance cursor past
// legitimate events. The sync cursor must only advance when validated
// against the locally-assigned seq after successful insert.
// ---------------------------------------------------------------------------

/// Helper: build a minimal `ArtistUpserted` event for seq-integrity tests.
fn make_artist_event(
    event_id: &str,
    artist_id: &str,
    seq: i64,
    now: i64,
) -> stophammer::event::Event {
    use stophammer::event::{
        ArtistUpsertedPayload, Event, EventPayload, EventType,
    };
    use stophammer::model::Artist;

    let artist = Artist {
        artist_id:  artist_id.into(),
        name:       format!("Artist {artist_id}"),
        name_lower: format!("artist {artist_id}"),
        sort_name:  None,
        type_id:    None,
        area:       None,
        img_url:    None,
        url:        None,
        begin_year: None,
        end_year:   None,
        created_at: now,
        updated_at: now,
    };
    let inner = ArtistUpsertedPayload { artist };
    let payload_json =
        serde_json::to_string(&inner).expect("serialize inner payload");

    Event {
        event_id:     event_id.into(),
        event_type:   EventType::ArtistUpserted,
        payload:      EventPayload::ArtistUpserted(inner),
        subject_guid: artist_id.into(),
        signed_by:    "deadbeef".into(),
        signature:    "cafebabe".into(),
        seq,
        created_at:   now,
        warnings:     vec![],
        payload_json,
    }
}

// ---------------------------------------------------------------------------
// 1. seq included in EventSigningPayload
// ---------------------------------------------------------------------------

/// The signing payload must include `seq` so that an attacker cannot
/// inflate the delivery-order cursor by changing the unsigned `seq` field.
// Issue-SEQ-INTEGRITY — 2026-03-14
#[test]
fn event_signing_payload_includes_seq() {
    use stophammer::event::{EventSigningPayload, EventType};

    let payload = EventSigningPayload {
        event_id:     "evt-1",
        event_type:   &EventType::ArtistUpserted,
        payload_json: "{}",
        subject_guid: "subj-1",
        created_at:   1000,
        seq:          42,
    };

    let json = serde_json::to_string(&payload).expect("serialize");
    assert!(
        json.contains("\"seq\":42"),
        "serialized EventSigningPayload must contain seq field: {json}"
    );
}

// ---------------------------------------------------------------------------
// 2. Changing seq breaks signature verification
// ---------------------------------------------------------------------------

/// If `seq` is covered by the signature, then changing `ev.seq` after
/// signing must cause `verify_event_signature` to fail.
// Issue-SEQ-INTEGRITY — 2026-03-14
#[test]
fn inflated_seq_breaks_signature() {
    use stophammer::event::{
        ArtistUpsertedPayload, Event, EventPayload, EventType,
    };
    use stophammer::model::Artist;
    use stophammer::signing::NodeSigner;

    let signer =
        NodeSigner::load_or_create("/tmp/seq-integrity-test.key").unwrap();

    let artist = Artist {
        artist_id:  "seq-artist-1".into(),
        name:       "Seq Artist".into(),
        name_lower: "seq artist".into(),
        sort_name:  None,
        type_id:    None,
        area:       None,
        img_url:    None,
        url:        None,
        begin_year: None,
        end_year:   None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
    };
    let inner = ArtistUpsertedPayload { artist };
    let payload_json = serde_json::to_string(&inner).unwrap();

    // Sign with the real seq=5.
    let (signed_by, signature) = signer.sign_event(
        "evt-seq-1",
        &EventType::ArtistUpserted,
        &payload_json,
        "seq-artist-1",
        1_000_000,
        5, // original seq
    );

    // Verify with seq=5 — must succeed.
    let good_event = Event {
        event_id:     "evt-seq-1".into(),
        event_type:   EventType::ArtistUpserted,
        payload:      EventPayload::ArtistUpserted(inner),
        payload_json,
        subject_guid: "seq-artist-1".into(),
        signed_by,
        signature,
        seq:          5,
        created_at:   1_000_000,
        warnings:     vec![],
    };
    stophammer::signing::verify_event_signature(&good_event)
        .expect("signature must verify with correct seq");

    // MITM inflates seq to 99999 — must FAIL verification.
    let bad_event = Event {
        seq: 99999,
        ..good_event
    };
    let result = stophammer::signing::verify_event_signature(&bad_event);
    assert!(
        result.is_err(),
        "inflated seq must break signature verification"
    );
}

// ---------------------------------------------------------------------------
// 3. apply_single_event uses ev.seq for cursor (after verification)
// ---------------------------------------------------------------------------

/// After a valid event is applied, the cursor in `node_sync_state` must
/// reflect `ev.seq` (the primary's seq, now signature-protected).
// Issue-SEQ-INTEGRITY — 2026-03-14
#[test]
fn apply_uses_wire_seq_for_cursor_after_verification() {
    use stophammer::apply::{apply_single_event, ApplyOutcome};

    let db: Arc<Mutex<rusqlite::Connection>> = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let now = common::now();

    let ev = make_artist_event("seq-cursor-evt-1", "seq-cursor-a1", 42, now);

    let result = apply_single_event(&pool, &ev);
    assert!(
        matches!(result, Ok(ApplyOutcome::Applied(_))),
        "apply must succeed: {result:?}"
    );

    // The cursor must be set to ev.seq (42), not a locally assigned value.
    let stored_seq: i64 = {
        let conn = db.lock().expect("lock");
        conn.query_row(
            "SELECT last_seq FROM node_sync_state WHERE node_pubkey = 'primary_sync_cursor'",
            [],
            |r| r.get(0),
        )
        .expect("cursor row must exist")
    };
    assert_eq!(
        stored_seq, 42,
        "cursor must reflect the primary's seq (ev.seq) which is now signature-protected"
    );
}

// ---------------------------------------------------------------------------
// 4. sign_event includes seq parameter
// ---------------------------------------------------------------------------

/// `NodeSigner::sign_event` must accept a `seq` parameter and produce
/// different signatures for different seq values.
// Issue-SEQ-INTEGRITY — 2026-03-14
#[test]
fn sign_event_produces_different_signatures_for_different_seq() {
    use stophammer::event::EventType;
    use stophammer::signing::NodeSigner;

    let signer =
        NodeSigner::load_or_create("/tmp/seq-integrity-diff.key").unwrap();

    let (_, sig_a) = signer.sign_event(
        "evt-diff", &EventType::ArtistUpserted, "{}", "subj", 1000, 1,
    );
    let (_, sig_b) = signer.sign_event(
        "evt-diff", &EventType::ArtistUpserted, "{}", "subj", 1000, 2,
    );

    assert_ne!(
        sig_a, sig_b,
        "different seq values must produce different signatures"
    );
}
