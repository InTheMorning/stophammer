//! ADR 0034 §11, task 002: no reader uses the artist credit.
//!
//! The internal SSE registry keys its channels by feed GUID, and the feed
//! and track quality scores read `release_artist`/`track_artist` in place
//! of `artist_credit_id`. This file is the mechanical acceptance suite for
//! that task. It changes no write and no event.

#![expect(
    clippy::too_many_lines,
    reason = "regression tests keep full ingest payloads and both fallback branches inline so the case stays explicit"
)]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use rusqlite::params;
use tower::ServiceExt;

use stophammer::event::{
    Event, EventPayload, EventType, FeedUpsertedPayload, LiveEventsReplacedPayload,
    TrackUpsertedPayload,
};
use stophammer::model::{Feed, LiveEvent, Track};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds a minimal, otherwise-empty feed for the given `feed_guid`.
fn sample_feed(feed_guid: &str) -> Feed {
    Feed {
        feed_guid: feed_guid.to_string(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
        title: "ADR 0034 Sample Feed".to_string(),
        title_lower: "adr 0034 sample feed".to_string(),
        artist_credit_id: Some(0),
        description: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: None,
        release_artist_sort: None,
        release_date: None,
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: 0,
        updated_at: 0,
        raw_medium: Some("music".to_string()),
        last_build_date: None,
        release_artist_source: None,
    }
}

/// Builds a minimal, otherwise-empty track for the given `feed_guid`/`track_guid`.
fn sample_track(feed_guid: &str, track_guid: &str) -> Track {
    Track {
        track_guid: track_guid.to_string(),
        feed_guid: feed_guid.to_string(),
        artist_credit_id: Some(0),
        title: "ADR 0034 Sample Track".to_string(),
        title_lower: "adr 0034 sample track".to_string(),
        pub_date: None,
        duration_secs: None,
        enclosure_url: None,
        enclosure_type: None,
        enclosure_bytes: None,
        track_number: None,
        season: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        description: None,
        track_artist: None,
        track_artist_sort: None,
        created_at: 0,
        updated_at: 0,
    }
}

/// Builds an unsigned event. `publish_events_to_sse` reads only `payload`,
/// `event_type` and `seq`, so the signature fields carry placeholder values.
fn sample_event(
    event_type: EventType,
    subject_guid: &str,
    payload: EventPayload,
    seq: i64,
) -> Event {
    Event {
        event_id: format!("ev-{subject_guid}"),
        event_type,
        payload,
        subject_guid: subject_guid.to_string(),
        signed_by: "deadbeef".to_string(),
        signature: "cafebabe".to_string(),
        seq,
        created_at: 0,
        warnings: vec![],
        payload_json: "{}".to_string(),
    }
}

/// Builds `AppState` wired with a crawl-token-only verifier chain, for
/// ingesting a feed through the real `/ingest/feed` handler.
fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0034-readers"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

fn json_request(method: &str, uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(body).expect("serialize")))
        .expect("build request")
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

// ---------------------------------------------------------------------------
// SSE: an event reaches the channel of the feed GUID it names
// ---------------------------------------------------------------------------

#[tokio::test]
async fn feed_upserted_event_reaches_its_feed_guid_channel() {
    let registry = stophammer::api::SseRegistry::new();
    let feed_guid = "feed-adr0034-upsert";

    let mut rx = registry
        .subscribe(feed_guid)
        .expect("subscribe should succeed");

    let ev = sample_event(
        EventType::FeedUpserted,
        feed_guid,
        EventPayload::FeedUpserted(FeedUpsertedPayload {
            feed: sample_feed(feed_guid),
            reason: None,
        }),
        1,
    );

    stophammer::api::publish_events_to_sse(&registry, &[ev]);

    let received = rx.try_recv().expect("subscriber should receive the frame");
    assert_eq!(received.event_type, "feed_upserted");
    assert_eq!(received.seq, 1);
}

#[tokio::test]
async fn track_upserted_event_reaches_the_feed_guid_channel_of_its_track() {
    let registry = stophammer::api::SseRegistry::new();
    let feed_guid = "feed-adr0034-track-upsert";
    let track_guid = "track-adr0034-upsert";

    let mut rx = registry
        .subscribe(feed_guid)
        .expect("subscribe should succeed");

    let ev = sample_event(
        EventType::TrackUpserted,
        track_guid,
        EventPayload::TrackUpserted(TrackUpsertedPayload {
            track: sample_track(feed_guid, track_guid),
            routes: vec![],
            value_time_splits: vec![],
        }),
        2,
    );

    stophammer::api::publish_events_to_sse(&registry, &[ev]);

    let received = rx.try_recv().expect("subscriber should receive the frame");
    assert_eq!(received.event_type, "track_upserted");
    assert_eq!(received.seq, 2);
}

#[tokio::test]
async fn live_frame_reaches_its_feed_guid_channel() {
    let registry = stophammer::api::SseRegistry::new();
    let feed_guid = "feed-adr0034-live";

    let mut rx = registry
        .subscribe(feed_guid)
        .expect("subscribe should succeed");

    let live_event = LiveEvent {
        live_item_guid: "live-item-adr0034".to_string(),
        feed_guid: feed_guid.to_string(),
        title: "ADR 0034 Live Item".to_string(),
        content_link: None,
        status: "live".to_string(),
        scheduled_start: None,
        scheduled_end: None,
        created_at: 0,
        updated_at: 0,
        live_value_uri: None,
        live_value_protocol: None,
    };

    let snapshot_ev = sample_event(
        EventType::LiveEventsReplaced,
        feed_guid,
        EventPayload::LiveEventsReplaced(LiveEventsReplacedPayload {
            feed_guid: feed_guid.to_string(),
            live_events: vec![live_event.clone()],
        }),
        7,
    );

    let frames = stophammer::api::build_live_sse_frames_for_feed(
        feed_guid,
        &[],
        &[live_event],
        &[snapshot_ev],
    );
    assert!(
        frames.iter().all(|(key, _)| key == feed_guid),
        "every live frame must be keyed by the feed GUID, got: {frames:?}"
    );
    assert!(
        frames
            .iter()
            .any(|(_, frame)| frame.event_type == "live_event_started"),
        "expected a live_event_started frame, got: {frames:?}"
    );

    stophammer::api::publish_sse_frames(&registry, &frames);

    let received = rx
        .try_recv()
        .expect("subscriber of the feed channel should receive the live frame");
    assert_eq!(received.event_type, "live_event_started");
    assert_eq!(received.subject_guid, "live-item-adr0034");
}

// ---------------------------------------------------------------------------
// Quality: the feed and track scores read release_artist/track_artist
// ---------------------------------------------------------------------------

#[tokio::test]
async fn feed_quality_score_matches_the_documented_rule() {
    let crawl_token = "adr0034-quality-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(Arc::clone(&state));
    let feed_guid = "feed-adr0034-quality";
    let track_guid = "track-adr0034-quality";

    let ingest_body = serde_json::json!({
        "canonical_url": "https://example.com/adr0034-quality-feed.xml",
        "source_url": "https://example.com/adr0034-quality-feed.xml",
        "http_status": 200,
        "content_hash": "adr0034-quality-hash",
        "crawl_token": crawl_token,
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "ADR 0034 Quality Feed",
            "description": "A feed for the ADR 0034 quality regression test",
            "image_url": "https://example.com/cover.jpg",
            "language": "en",
            "explicit": true,
            "itunes_type": "episodic",
            "owner_name": "ADR Test Artist",
            "raw_medium": "music",
            "tracks": [
                {
                    "track_guid": track_guid,
                    "title": "ADR 0034 Quality Track",
                    "pub_date": 1_700_000_000,
                    "explicit": false,
                    "payment_routes": [
                        {
                            "recipient_name": "Artist",
                            "route_type": "keysend",
                            "address": "02abc123",
                            "split": 100,
                            "fee": false
                        }
                    ],
                    "value_time_splits": []
                }
            ],
            "remote_items": [],
            "feed_payment_routes": [],
            "live_items": []
        }
    });

    let req = json_request("POST", "/ingest/feed", &ingest_body);
    let resp = app.oneshot(req).await.expect("ingest");
    assert_eq!(resp.status(), 200, "ingest should succeed");
    let body = body_json(resp).await;
    assert!(
        body["accepted"].as_bool().unwrap_or(false),
        "ingest should be accepted: {body}"
    );

    // ADR 0034 §11: the scoring rules (src/quality.rs `compute_feed_quality`
    // doc comment), computed from what the request above sets, not from the
    // implementation:
    //   title(10) + description(15) + image_url(15) + language(5) +
    //   episode_count(5) + release_artist(10) + newest_item_at(5) +
    //   explicit(5) + itunes_type(5) + has_tracks(10) + has_routes(15) = 100
    let expected_score = 100;

    let conn = db.lock().unwrap();
    let score = stophammer::quality::compute_feed_quality(&conn, feed_guid)
        .expect("compute_feed_quality should succeed");
    assert_eq!(
        score, expected_score,
        "feed quality score must match the documented release_artist-based rule"
    );

    // The ingest path always derives a non-empty release_artist (at minimum
    // the "Unknown Artist" placeholder), so this feed's release_artist bonus
    // matches what the retired artist_credit_id > 0 rule would have given
    // for the same ingest, since ingest always creates a credit row too.
    let release_artist: Option<String> = conn
        .query_row(
            "SELECT release_artist FROM feeds WHERE feed_guid = ?1",
            params![feed_guid],
            |row| row.get(0),
        )
        .expect("feed row should exist");
    assert_eq!(release_artist.as_deref(), Some("ADR Test Artist"));
}

#[tokio::test]
async fn track_quality_score_reads_track_artist_then_falls_back_to_release_artist() {
    let crawl_token = "adr0034-track-quality-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(Arc::clone(&state));
    let feed_guid = "feed-adr0034-track-quality";
    let track_guid = "track-adr0034-track-quality";

    let ingest_body = serde_json::json!({
        "canonical_url": "https://example.com/adr0034-track-quality-feed.xml",
        "source_url": "https://example.com/adr0034-track-quality-feed.xml",
        "http_status": 200,
        "content_hash": "adr0034-track-quality-hash",
        "crawl_token": crawl_token,
        "feed_data": {
            "feed_guid": feed_guid,
            "title": "ADR 0034 Track Quality Feed",
            "owner_name": "ADR Track Test Artist",
            "explicit": false,
            "raw_medium": "music",
            "tracks": [
                {
                    "track_guid": track_guid,
                    "title": "ADR 0034 Track Quality Track",
                    "pub_date": 1_700_000_000,
                    "duration_secs": 180,
                    "enclosure_url": "https://example.com/track.mp3",
                    "enclosure_type": "audio/mpeg",
                    "track_number": 1,
                    "season": 1,
                    "description": "A track for the ADR 0034 quality regression test",
                    "author_name": "Track Specific Artist",
                    "explicit": false,
                    "payment_routes": [
                        {
                            "recipient_name": "Artist",
                            "route_type": "keysend",
                            "address": "02abc123",
                            "split": 100,
                            "fee": false
                        }
                    ],
                    "value_time_splits": [
                        {
                            "start_time_secs": 0,
                            "remote_feed_guid": "remote-feed-adr0034",
                            "remote_item_guid": "remote-item-adr0034",
                            "split": 50
                        }
                    ]
                }
            ],
            "remote_items": [],
            "feed_payment_routes": [],
            "live_items": []
        }
    });

    let req = json_request("POST", "/ingest/feed", &ingest_body);
    let resp = app.oneshot(req).await.expect("ingest");
    assert_eq!(resp.status(), 200, "ingest should succeed");
    let body = body_json(resp).await;
    assert!(
        body["accepted"].as_bool().unwrap_or(false),
        "ingest should be accepted: {body}"
    );

    // ADR 0034 §11: the scoring rules (src/quality.rs
    // `compute_track_quality_for_feed_track` doc comment), computed from
    // what the request above sets, not from the implementation:
    //   title(10) + enclosure_url(15) + enclosure_type(5) + duration(10) +
    //   pub_date(5) + description(10) + track_artist(5) + track_number(5) +
    //   season(5) + routes(20) + value_time_splits(10) = 100
    let expected_score = 100;

    {
        let conn = db.lock().unwrap();
        let track_artist: Option<String> = conn
            .query_row(
                "SELECT track_artist FROM tracks WHERE feed_guid = ?1 AND track_guid = ?2",
                params![feed_guid, track_guid],
                |row| row.get(0),
            )
            .expect("track row should exist");
        assert_eq!(
            track_artist.as_deref(),
            Some("Track Specific Artist"),
            "the track's own author_name should have become its track_artist"
        );

        let score =
            stophammer::quality::compute_track_quality_for_feed_track(&conn, feed_guid, track_guid)
                .expect("compute_track_quality_for_feed_track should succeed");
        assert_eq!(
            score, expected_score,
            "track quality score must match the documented track_artist-based rule"
        );
    }

    // Clear the track's own track_artist. The feed's release_artist
    // ("ADR Track Test Artist") is still non-empty, so the track score must
    // stay the same through the fallback rule.
    {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE tracks SET track_artist = NULL WHERE feed_guid = ?1 AND track_guid = ?2",
            params![feed_guid, track_guid],
        )
        .expect("clear track_artist");

        let release_artist: Option<String> = conn
            .query_row(
                "SELECT release_artist FROM feeds WHERE feed_guid = ?1",
                params![feed_guid],
                |row| row.get(0),
            )
            .expect("feed row should exist");
        assert_eq!(release_artist.as_deref(), Some("ADR Track Test Artist"));

        let score =
            stophammer::quality::compute_track_quality_for_feed_track(&conn, feed_guid, track_guid)
                .expect("compute_track_quality_for_feed_track should succeed");
        assert_eq!(
            score, expected_score,
            "a track with no track_artist must still earn the bonus from its feed's release_artist"
        );
    }
}
