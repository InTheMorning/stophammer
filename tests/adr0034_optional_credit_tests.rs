//! ADR 0034 §11 (Release A) integration tests.
//!
//! A node applies a signed `FeedUpserted` or `TrackUpserted` event whose
//! payload has no `artist_credit_id`. The node makes the feed-scoped credit
//! from the payload text itself, the way ingest does, and stores it. These
//! tests exercise `stophammer::apply::apply_single_event` directly, the same
//! entry point a community node uses for a pushed event.

#![allow(
    clippy::clone_on_ref_ptr,
    reason = "tests use an Arc-backed in-memory DB handle; wrap_pool takes it by value, so the caller keeps a clone to read back after apply"
)]

mod common;

use stophammer::apply::ApplyOutcome;
use stophammer::event::{
    Event, EventPayload, EventType, FeedUpsertedPayload, TrackUpsertedPayload,
};
use stophammer::model::{Feed, Track};
use stophammer::signing::NodeSigner;

fn make_feed(
    feed_guid: &str,
    release_artist: Option<&str>,
    artist_credit_id: Option<i64>,
    now: i64,
) -> Feed {
    Feed {
        feed_guid: feed_guid.into(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
        title: "Test Feed".into(),
        title_lower: "test feed".into(),
        artist_credit_id,
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: release_artist.map(str::to_string),
        release_artist_sort: None,
        release_date: None,
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: now,
        updated_at: now,
        raw_medium: None,
        last_build_date: None,
        release_artist_source: None,
    }
}

fn make_track(
    track_guid: &str,
    feed_guid: &str,
    track_artist: Option<&str>,
    artist_credit_id: Option<i64>,
    now: i64,
) -> Track {
    Track {
        track_guid: track_guid.into(),
        feed_guid: feed_guid.into(),
        artist_credit_id,
        title: "Test Track".into(),
        title_lower: "test track".into(),
        pub_date: Some(now),
        duration_secs: Some(180),
        enclosure_url: Some(format!("https://cdn.example.com/{track_guid}.mp3")),
        enclosure_type: Some("audio/mpeg".into()),
        enclosure_bytes: Some(1_000_000),
        track_number: None,
        season: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        description: None,
        track_artist: track_artist.map(str::to_string),
        track_artist_sort: None,
        created_at: now,
        updated_at: now,
    }
}

/// Builds a signed `FeedUpserted` event, the way a primary or an upstream
/// node signs one before pushing it to a community node.
fn sign_feed_upserted(
    signer: &NodeSigner,
    event_id: &str,
    feed: Feed,
    now: i64,
    seq: i64,
) -> Event {
    let feed_guid = feed.feed_guid.clone();
    let inner = FeedUpsertedPayload { feed, reason: None };
    let payload_json = serde_json::to_string(&inner).expect("serialize FeedUpsertedPayload");
    let (signed_by, signature) = signer.sign_event(
        event_id,
        &EventType::FeedUpserted,
        &payload_json,
        &feed_guid,
        now,
        seq,
    );
    Event {
        event_id: event_id.into(),
        event_type: EventType::FeedUpserted,
        payload: EventPayload::FeedUpserted(inner),
        subject_guid: feed_guid,
        signed_by,
        signature,
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    }
}

/// Builds a signed `TrackUpserted` event, the way a primary or an upstream
/// node signs one before pushing it to a community node.
fn sign_track_upserted(
    signer: &NodeSigner,
    event_id: &str,
    track: Track,
    now: i64,
    seq: i64,
) -> Event {
    let track_guid = track.track_guid.clone();
    let inner = TrackUpsertedPayload {
        track,
        routes: vec![],
        value_time_splits: vec![],
    };
    let payload_json = serde_json::to_string(&inner).expect("serialize TrackUpsertedPayload");
    let (signed_by, signature) = signer.sign_event(
        event_id,
        &EventType::TrackUpserted,
        &payload_json,
        &track_guid,
        now,
        seq,
    );
    Event {
        event_id: event_id.into(),
        event_type: EventType::TrackUpserted,
        payload: EventPayload::TrackUpserted(inner),
        subject_guid: track_guid,
        signed_by,
        signature,
        seq,
        created_at: now,
        warnings: vec![],
        payload_json,
    }
}

// ---------------------------------------------------------------------------
// 1. A FeedUpserted event with no credit makes one from release_artist.
// ---------------------------------------------------------------------------

#[test]
fn feed_upserted_with_no_credit_makes_release_artist_credit() {
    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let signer = common::temp_signer("adr0034-feed-credit");
    let now = common::now();

    let feed = make_feed("feed-adr0034-a", Some("  Some Artist  "), None, now);
    let ev = sign_feed_upserted(&signer, "evt-adr0034-feed-a", feed, now, 1);

    let outcome = stophammer::apply::apply_single_event(&pool, &ev)
        .expect("a FeedUpserted event with no artist_credit_id must apply");
    assert!(
        matches!(outcome, ApplyOutcome::Applied(_)),
        "expected the event to be newly applied, got {outcome:?}"
    );

    let conn = db.lock().unwrap();
    let stored_feed = stophammer::db::get_feed_by_guid(&conn, "feed-adr0034-a")
        .expect("read stored feed")
        .expect("feed row must exist after apply");
    let credit_id = stored_feed
        .artist_credit_id
        .expect("apply must resolve artist_credit_id to Some before the upsert");
    let credit = stophammer::db::get_artist_credit(&conn, credit_id)
        .expect("read stored credit")
        .expect("credit row must exist");
    assert_eq!(
        credit.display_name, "Some Artist",
        "the made credit must carry the trimmed release_artist"
    );
}

// ---------------------------------------------------------------------------
// 2. A FeedUpserted event with no credit and no release_artist gets the
//    placeholder credit "Unknown Artist".
// ---------------------------------------------------------------------------

#[test]
fn feed_upserted_with_no_credit_and_no_release_artist_uses_placeholder() {
    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let signer = common::temp_signer("adr0034-feed-placeholder");
    let now = common::now();

    let feed = make_feed("feed-adr0034-b", None, None, now);
    let ev = sign_feed_upserted(&signer, "evt-adr0034-feed-b", feed, now, 1);

    stophammer::apply::apply_single_event(&pool, &ev)
        .expect("a FeedUpserted event with no artist_credit_id and no release_artist must apply");

    let conn = db.lock().unwrap();
    let stored_feed = stophammer::db::get_feed_by_guid(&conn, "feed-adr0034-b")
        .expect("read stored feed")
        .expect("feed row must exist after apply");
    let credit_id = stored_feed
        .artist_credit_id
        .expect("apply must resolve artist_credit_id to Some before the upsert");
    let credit = stophammer::db::get_artist_credit(&conn, credit_id)
        .expect("read stored credit")
        .expect("credit row must exist");
    assert_eq!(
        credit.display_name, "Unknown Artist",
        "an absent release_artist must give the placeholder credit"
    );
}

// ---------------------------------------------------------------------------
// 3. A TrackUpserted event with no credit makes one from track_artist.
// ---------------------------------------------------------------------------

#[test]
fn track_upserted_with_no_credit_uses_track_artist() {
    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let signer = common::temp_signer("adr0034-track-credit");
    let now = common::now();

    let feed = make_feed("feed-adr0034-c", Some("Feed Artist For Track"), None, now);
    let feed_ev = sign_feed_upserted(&signer, "evt-adr0034-feed-c", feed, now, 1);
    stophammer::apply::apply_single_event(&pool, &feed_ev).expect("apply prerequisite feed");

    let track = make_track(
        "track-adr0034-c1",
        "feed-adr0034-c",
        Some("  Track Artist  "),
        None,
        now,
    );
    let track_ev = sign_track_upserted(&signer, "evt-adr0034-track-c1", track, now, 2);

    let outcome = stophammer::apply::apply_single_event(&pool, &track_ev)
        .expect("a TrackUpserted event with no artist_credit_id must apply");
    assert!(
        matches!(outcome, ApplyOutcome::Applied(_)),
        "expected the event to be newly applied, got {outcome:?}"
    );

    let conn = db.lock().unwrap();
    let stored_track =
        stophammer::db::get_track_for_feed(&conn, "feed-adr0034-c", "track-adr0034-c1")
            .expect("read stored track")
            .expect("track row must exist after apply");
    let credit_id = stored_track
        .artist_credit_id
        .expect("apply must resolve artist_credit_id to Some before the upsert");
    let credit = stophammer::db::get_artist_credit(&conn, credit_id)
        .expect("read stored credit")
        .expect("credit row must exist");
    assert_eq!(
        credit.display_name, "Track Artist",
        "the made credit must carry the trimmed track_artist"
    );
}

// ---------------------------------------------------------------------------
// 4. A TrackUpserted event with no credit and no track_artist gets the
//    feed's own stored credit.
// ---------------------------------------------------------------------------

#[test]
fn track_upserted_with_no_credit_and_no_track_artist_uses_feed_credit() {
    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let signer = common::temp_signer("adr0034-track-feed-credit");
    let now = common::now();

    let feed = make_feed(
        "feed-adr0034-d",
        Some("Feed Artist For Track Fallback"),
        None,
        now,
    );
    let feed_ev = sign_feed_upserted(&signer, "evt-adr0034-feed-d", feed, now, 1);
    stophammer::apply::apply_single_event(&pool, &feed_ev).expect("apply prerequisite feed");

    let track = make_track("track-adr0034-d1", "feed-adr0034-d", None, None, now);
    let track_ev = sign_track_upserted(&signer, "evt-adr0034-track-d1", track, now, 2);

    stophammer::apply::apply_single_event(&pool, &track_ev)
        .expect("a TrackUpserted event with no artist_credit_id and no track_artist must apply");

    let conn = db.lock().unwrap();
    let stored_feed = stophammer::db::get_feed_by_guid(&conn, "feed-adr0034-d")
        .expect("read stored feed")
        .expect("feed row must exist after apply");
    let stored_track =
        stophammer::db::get_track_for_feed(&conn, "feed-adr0034-d", "track-adr0034-d1")
            .expect("read stored track")
            .expect("track row must exist after apply");

    assert_eq!(
        stored_track.artist_credit_id, stored_feed.artist_credit_id,
        "a track with no track_artist must take the feed's own stored credit"
    );
}

// ---------------------------------------------------------------------------
// 5. A Feed with Some(7) serializes the same way it did before this change.
// ---------------------------------------------------------------------------

#[test]
fn feed_with_some_credit_id_serializes_same_as_before() {
    let now = common::now();
    let feed = make_feed("feed-adr0034-e", Some("Some Artist"), Some(7), now);

    let json = serde_json::to_string(&feed).expect("serialize feed");
    assert!(
        json.contains("\"artist_credit_id\":7"),
        "a Feed with Some(7) must serialize the key artist_credit_id with the value 7: {json}"
    );
}
