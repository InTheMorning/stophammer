//! ADR 0034 §11, task 003: the ingest stops the credit, and the tables go.
//!
//! This is the mechanical acceptance suite named by
//! `docs/tasks/adr-0034-task-003-stop-the-credit.md`. It proves three of the
//! task's acceptance criteria that `tests/migration_tests.rs` does not
//! already cover:
//!
//! - An ingest of a feed with two tracks emits no `artist_upserted` and no
//!   `artist_credit_created` event.
//! - A node applies a log of signed events: `ArtistUpserted`,
//!   `ArtistCreditCreated`, then a `FeedUpserted` and a `TrackUpserted` whose
//!   JSON has `artist_credit_id`. Each event counts as applied, and the feed
//!   and the track are stored. This also proves the model types have no
//!   `deny_unknown_fields`: an old payload with the `artist_credit_id` key
//!   still deserializes.
//! - A fresh database from `schema.sql` has no artist table and no
//!   `artist_credit_id` column.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

use stophammer::event::{
    ArtistCreditCreatedPayload, ArtistUpsertedPayload, Event, EventPayload, EventType,
    FeedUpsertedPayload, TrackUpsertedPayload,
};
use stophammer::model::{Artist, ArtistCredit, ArtistCreditName, Feed, Track};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds `AppState` wired with a crawl-token-only verifier chain, for
/// ingesting a feed through the real `/ingest/feed` handler.
fn test_app_state_with_crawl_token(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0034-artist-credit-removed"));
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

async fn ingest_accepted(app: axum::Router, payload: &serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

// ---------------------------------------------------------------------------
// 1. An ingest of a feed with two tracks emits no artist_upserted and no
//    artist_credit_created event.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ingest_of_a_two_track_feed_emits_no_artist_events() {
    let crawl_token = "adr0034-removed-ingest-token";
    let db = common::test_db_arc();
    let state = test_app_state_with_crawl_token(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(Arc::clone(&state));

    let payload = serde_json::json!({
        "canonical_url": "https://example.com/adr0034-removed-feed.xml",
        "source_url": "https://example.com/adr0034-removed-feed.xml",
        "http_status": 200,
        "content_hash": "adr0034-removed-hash",
        "crawl_token": crawl_token,
        "feed_data": {
            "feed_guid": "feed-adr0034-removed",
            "title": "ADR 0034 Removed Credit Feed",
            "owner_name": "ADR 0034 Removed Credit Artist",
            "explicit": false,
            "raw_medium": "music",
            "tracks": [
                {
                    "track_guid": "track-adr0034-removed-1",
                    "title": "Track One",
                    "pub_date": 1_700_000_000,
                    "explicit": false
                },
                {
                    "track_guid": "track-adr0034-removed-2",
                    "title": "Track Two",
                    "pub_date": 1_700_000_100,
                    "explicit": false
                }
            ]
        }
    });

    ingest_accepted(app, &payload).await;

    let conn = db.lock().expect("lock db");

    let track_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tracks WHERE feed_guid = 'feed-adr0034-removed'",
            [],
            |r| r.get(0),
        )
        .expect("count tracks");
    assert_eq!(track_count, 2, "both tracks must be stored");

    let artist_upserted_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'artist_upserted'",
            [],
            |r| r.get(0),
        )
        .expect("count artist_upserted events");
    assert_eq!(
        artist_upserted_count, 0,
        "the ingest must emit no artist_upserted event"
    );

    let artist_credit_created_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'artist_credit_created'",
            [],
            |r| r.get(0),
        )
        .expect("count artist_credit_created events");
    assert_eq!(
        artist_credit_created_count, 0,
        "the ingest must emit no artist_credit_created event"
    );
}

// ---------------------------------------------------------------------------
// 2. A node applies a log of signed events: ArtistUpserted,
//    ArtistCreditCreated, then a FeedUpserted and a TrackUpserted whose JSON
//    has artist_credit_id. Each event counts as applied, and the feed and
//    the track are stored.
// ---------------------------------------------------------------------------

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "builds four full signed events inline so the applied log stays readable in one place"
)]
async fn applying_a_log_with_old_artist_events_then_a_legacy_feed_and_track_stores_both() {
    let (pool, _dir) = common::test_db_pool();
    let signer = common::temp_signer("adr0034-removed-apply");

    // Event 1: ArtistUpserted, from before release 0.3.0.
    let artist = Artist {
        artist_id: "artist-legacy".into(),
        name: "Legacy Artist".into(),
        name_lower: "legacy artist".into(),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
    };
    let artist_payload_json =
        serde_json::to_string(&ArtistUpsertedPayload { artist }).expect("serialize");
    let (signed_by, signature) = signer.sign_event(
        "evt-legacy-artist",
        &EventType::ArtistUpserted,
        &artist_payload_json,
        "artist-legacy",
        1_000_000,
        1,
    );
    let ev_artist = Event {
        event_id: "evt-legacy-artist".into(),
        event_type: EventType::ArtistUpserted,
        payload: EventPayload::ArtistUpserted(ArtistUpsertedPayload {
            artist: Artist {
                artist_id: "artist-legacy".into(),
                name: "Legacy Artist".into(),
                name_lower: "legacy artist".into(),
                sort_name: None,
                type_id: None,
                area: None,
                img_url: None,
                url: None,
                begin_year: None,
                end_year: None,
                created_at: 1_000_000,
                updated_at: 1_000_000,
            },
        }),
        payload_json: artist_payload_json,
        subject_guid: "artist-legacy".into(),
        signed_by,
        signature,
        seq: 1,
        created_at: 1_000_000,
        warnings: vec![],
    };

    // Event 2: ArtistCreditCreated, from before release 0.3.0.
    let artist_credit = ArtistCredit {
        id: 1,
        display_name: "Legacy Artist".into(),
        feed_guid: None,
        created_at: 1_000_000,
        names: vec![ArtistCreditName {
            id: 1,
            artist_credit_id: 1,
            artist_id: "artist-legacy".into(),
            position: 0,
            name: "Legacy Artist".into(),
            join_phrase: String::new(),
        }],
    };
    let credit_payload_json =
        serde_json::to_string(&ArtistCreditCreatedPayload { artist_credit }).expect("serialize");
    let (signed_by, signature) = signer.sign_event(
        "evt-legacy-credit",
        &EventType::ArtistCreditCreated,
        &credit_payload_json,
        "artist-legacy",
        1_000_000,
        2,
    );
    let ev_credit = Event {
        event_id: "evt-legacy-credit".into(),
        event_type: EventType::ArtistCreditCreated,
        payload: EventPayload::ArtistCreditCreated(ArtistCreditCreatedPayload {
            artist_credit: ArtistCredit {
                id: 1,
                display_name: "Legacy Artist".into(),
                feed_guid: None,
                created_at: 1_000_000,
                names: vec![],
            },
        }),
        payload_json: credit_payload_json,
        subject_guid: "artist-legacy".into(),
        signed_by,
        signature,
        seq: 2,
        created_at: 1_000_000,
        warnings: vec![],
    };

    // Event 3: FeedUpserted, with a legacy payload_json that still carries
    // the artist_credit_id key from before this task. Feed and Track have no
    // deny_unknown_fields, so the extra key must not stop deserialization.
    let feed_payload_json = r#"{
        "feed": {
            "feed_guid": "feed-legacy-payload",
            "feed_url": "https://example.com/feed-legacy-payload.xml",
            "title": "Legacy Payload Feed",
            "title_lower": "legacy payload feed",
            "artist_credit_id": 1,
            "description": null,
            "image_url": null,
            "publisher": null,
            "language": null,
            "explicit": false,
            "itunes_type": null,
            "release_artist": null,
            "release_artist_sort": null,
            "release_date": null,
            "release_kind": null,
            "episode_count": 1,
            "newest_item_at": null,
            "oldest_item_at": null,
            "created_at": 1000000,
            "updated_at": 1000000,
            "raw_medium": "music"
        }
    }"#;
    let (signed_by, signature) = signer.sign_event(
        "evt-legacy-feed",
        &EventType::FeedUpserted,
        feed_payload_json,
        "feed-legacy-payload",
        1_000_000,
        3,
    );
    // `apply_single_event_inner` re-derives the applied payload from
    // `payload_json` (Issue-PAYLOAD-INTEGRITY), so this `payload` field is
    // not read on this path. It carries a same-shaped `FeedUpserted` value,
    // without the legacy `artist_credit_id` key, matching what a current
    // node would build from `payload_json` if it re-parsed it directly.
    let feed_placeholder = Feed {
        feed_guid: "feed-legacy-payload".into(),
        feed_url: "https://example.com/feed-legacy-payload.xml".into(),
        title: "Legacy Payload Feed".into(),
        title_lower: "legacy payload feed".into(),
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
        episode_count: 1,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: Some("music".into()),
        last_build_date: None,
        release_artist_source: None,
    };
    let ev_feed = Event {
        event_id: "evt-legacy-feed".into(),
        event_type: EventType::FeedUpserted,
        payload: EventPayload::FeedUpserted(FeedUpsertedPayload {
            feed: feed_placeholder,
            reason: None,
        }),
        payload_json: feed_payload_json.to_string(),
        subject_guid: "feed-legacy-payload".into(),
        signed_by,
        signature,
        seq: 3,
        created_at: 1_000_000,
        warnings: vec![],
    };

    // Event 4: TrackUpserted, with the same legacy artist_credit_id key,
    // referencing the feed applied above.
    let track_payload_json = r#"{
        "track": {
            "track_guid": "track-legacy-payload",
            "feed_guid": "feed-legacy-payload",
            "title": "Legacy Payload Track",
            "title_lower": "legacy payload track",
            "artist_credit_id": 1,
            "pub_date": 1000000,
            "duration_secs": null,
            "enclosure_url": null,
            "enclosure_type": null,
            "enclosure_bytes": null,
            "track_number": null,
            "season": null,
            "image_url": null,
            "publisher": null,
            "language": null,
            "explicit": false,
            "description": null,
            "track_artist": null,
            "track_artist_sort": null,
            "created_at": 1000000,
            "updated_at": 1000000
        },
        "routes": [],
        "value_time_splits": []
    }"#;
    let (signed_by, signature) = signer.sign_event(
        "evt-legacy-track",
        &EventType::TrackUpserted,
        track_payload_json,
        "track-legacy-payload",
        1_000_000,
        4,
    );
    // As above: `payload` is not read on this path. It carries a
    // same-shaped `TrackUpserted` value, without the legacy
    // `artist_credit_id` key.
    let track_placeholder = Track {
        track_guid: "track-legacy-payload".into(),
        feed_guid: "feed-legacy-payload".into(),
        title: "Legacy Payload Track".into(),
        title_lower: "legacy payload track".into(),
        pub_date: Some(1_000_000),
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
        created_at: 1_000_000,
        updated_at: 1_000_000,
    };
    let ev_track = Event {
        event_id: "evt-legacy-track".into(),
        event_type: EventType::TrackUpserted,
        payload: EventPayload::TrackUpserted(TrackUpsertedPayload {
            track: track_placeholder,
            routes: vec![],
            value_time_splits: vec![],
        }),
        payload_json: track_payload_json.to_string(),
        subject_guid: "track-legacy-payload".into(),
        signed_by,
        signature,
        seq: 4,
        created_at: 1_000_000,
        warnings: vec![],
    };

    for ev in [&ev_artist, &ev_credit, &ev_feed, &ev_track] {
        let outcome = stophammer::apply::apply_single_event(&pool, ev)
            .unwrap_or_else(|e| panic!("apply {} should succeed: {e}", ev.event_id));
        assert!(
            matches!(outcome, stophammer::apply::ApplyOutcome::Applied(_)),
            "{} must count as applied",
            ev.event_id
        );
    }

    let conn = pool.reader().expect("reader connection");

    let feed_title: String = conn
        .query_row(
            "SELECT title FROM feeds WHERE feed_guid = 'feed-legacy-payload'",
            [],
            |r| r.get(0),
        )
        .expect("the feed must be stored");
    assert_eq!(feed_title, "Legacy Payload Feed");

    let track_title: String = conn
        .query_row(
            "SELECT title FROM tracks WHERE track_guid = 'track-legacy-payload'",
            [],
            |r| r.get(0),
        )
        .expect("the track must be stored");
    assert_eq!(track_title, "Legacy Payload Track");
}

// ---------------------------------------------------------------------------
// 3. A fresh database from schema.sql has no artist table and no
//    artist_credit_id column.
// ---------------------------------------------------------------------------

#[test]
fn fresh_database_from_schema_sql_has_no_artist_table_and_no_credit_column() {
    let conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
    conn.execute_batch(include_str!("../src/schema.sql"))
        .expect("apply schema.sql");

    let dropped_tables = [
        "artists",
        "artist_aliases",
        "artist_credit",
        "artist_credit_name",
        "artist_type",
        "rel_type",
        "external_ids",
    ];
    for table in dropped_tables {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .expect("query sqlite_master");
        assert!(!exists, "schema.sql must not declare table {table}");
    }

    for table in ["feeds", "tracks"] {
        let has_credit_column: bool = conn
            .prepare(&format!("SELECT * FROM pragma_table_info('{table}')"))
            .expect("prepare pragma_table_info")
            .query_map([], |r| r.get::<_, String>(1))
            .expect("query table info")
            .filter_map(Result::ok)
            .any(|column| column == "artist_credit_id");
        assert!(
            !has_credit_column,
            "{table} must not declare an artist_credit_id column"
        );
    }
}
