// ADR 0084 §8, task 001: a track stores its sort key.
//
// Checks that `tracks.title_sort` equals `model::sort_key(title)`:
// - after the fill in `open_db` sets a null value,
// - after `db::upsert_track`, for an insert and for an update,
// - after an applied signed `TrackUpserted` event, which calls `upsert_track`,
// - after `POST /ingest/feed`, which writes through `ingest_transaction`.
// The event format does not change: `Track` has no `title_sort` field.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use http::Request;
use rusqlite::Connection;
use tower::ServiceExt;

use stophammer::event::{
    Event, EventPayload, EventType, FeedUpsertedPayload, TrackUpsertedPayload,
};
use stophammer::model::{Feed, Track, sort_key};

const MESSY_TITLE: &str = "  The  \u{c9}lan\tSession ";

fn sample_feed(feed_guid: &str) -> Feed {
    Feed {
        feed_guid: feed_guid.into(),
        feed_url: format!("https://example.com/{feed_guid}.xml"),
        title: "Sort Feed".into(),
        title_lower: "sort feed".into(),
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
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: None,
        last_build_date: None,
        release_artist_source: None,
    }
}

fn sample_track(feed_guid: &str, track_guid: &str, title: &str) -> Track {
    Track {
        track_guid: track_guid.into(),
        feed_guid: feed_guid.into(),
        title: title.into(),
        title_lower: title.to_lowercase(),
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
        created_at: 1_000_000,
        updated_at: 1_000_000,
    }
}

fn stored_sort(conn: &Connection, track_guid: &str) -> Option<String> {
    conn.query_row(
        "SELECT title_sort FROM tracks WHERE track_guid = ?1",
        [track_guid],
        |r| r.get(0),
    )
    .expect("the track row must exist")
}

#[test]
fn fill_sets_each_null_title_sort_on_open() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("fill.db");
    let path_str = path.to_str().expect("utf-8 path");

    let conn = stophammer::db::open_db(path_str);
    stophammer::db::upsert_feed(&conn, &sample_feed("fill-feed")).expect("upsert feed");
    for (guid, title) in [("fill-a", MESSY_TITLE), ("fill-b", "Plain")] {
        stophammer::db::upsert_track(&conn, &sample_track("fill-feed", guid, title))
            .expect("upsert track");
    }
    conn.execute("UPDATE tracks SET title_sort = NULL", [])
        .expect("clear the column");
    drop(conn);

    let conn = stophammer::db::open_db(path_str);
    for (guid, title) in [("fill-a", MESSY_TITLE), ("fill-b", "Plain")] {
        assert_eq!(
            stored_sort(&conn, guid),
            Some(sort_key(title)),
            "ADR 0084 §8: the fill in open_db must set tracks.title_sort to sort_key(title). \
             Fix ensure_track_title_sort_filled in src/db.rs."
        );
    }
}

#[test]
fn upsert_track_derives_title_sort_for_insert_and_update() {
    let conn = common::test_db();
    stophammer::db::upsert_feed(&conn, &sample_feed("up-feed")).expect("upsert feed");

    stophammer::db::upsert_track(&conn, &sample_track("up-feed", "up-a", MESSY_TITLE))
        .expect("insert track");
    assert_eq!(
        stored_sort(&conn, "up-a"),
        Some(sort_key(MESSY_TITLE)),
        "ADR 0084 §8: an insert through upsert_track must derive title_sort from title."
    );

    stophammer::db::upsert_track(&conn, &sample_track("up-feed", "up-a", "Changed  Title"))
        .expect("update track");
    assert_eq!(
        stored_sort(&conn, "up-a"),
        Some("changed title".to_string()),
        "ADR 0084 §8: an update through upsert_track must derive title_sort again. \
         Add title_sort to the DO UPDATE clause in src/db.rs."
    );
}

#[test]
fn applied_track_upserted_event_derives_title_sort() {
    let pool = common::test_db_pool_memory();
    let signer = common::temp_signer("adr0084-sort-apply-signer");
    let feed_guid = "ev-feed";
    let track_guid = "ev-track";

    let feed_payload = FeedUpsertedPayload {
        feed: sample_feed(feed_guid),
        reason: None,
    };
    let track_payload = TrackUpsertedPayload {
        track: sample_track(feed_guid, track_guid, MESSY_TITLE),
        routes: vec![],
        value_time_splits: vec![],
    };
    let feed_json = serde_json::to_string(&feed_payload).expect("feed json");
    let track_json = serde_json::to_string(&track_payload).expect("track json");

    let make =
        |id: &str, ty: EventType, payload: EventPayload, json: String, subject: &str, seq| {
            let (signed_by, signature) = signer.sign_event(id, &ty, &json, subject, 1_000_000, seq);
            Event {
                event_id: id.into(),
                event_type: ty,
                payload,
                payload_json: json,
                subject_guid: subject.into(),
                signed_by,
                signature,
                seq,
                created_at: 1_000_000,
                warnings: vec![],
            }
        };
    let events = [
        make(
            "ev-sort-feed",
            EventType::FeedUpserted,
            EventPayload::FeedUpserted(feed_payload),
            feed_json,
            feed_guid,
            1,
        ),
        make(
            "ev-sort-track",
            EventType::TrackUpserted,
            EventPayload::TrackUpserted(track_payload),
            track_json,
            track_guid,
            2,
        ),
    ];
    for ev in &events {
        stophammer::apply::apply_single_event(&pool, ev)
            .unwrap_or_else(|e| panic!("apply {} must succeed: {e}", ev.event_id));
    }

    let conn = pool.reader().expect("reader connection");
    assert_eq!(
        stored_sort(&conn, track_guid),
        Some(sort_key(MESSY_TITLE)),
        "ADR 0084 §8: a node that applies a signed TrackUpserted event must derive \
         title_sort itself. The event format does not carry it."
    );
}

#[tokio::test]
async fn ingest_feed_derives_title_sort_for_insert_and_update() {
    let crawl_token = "adr0084-sort-crawl-token";
    let signer = Arc::new(common::temp_signer("adr0084-sort-ingest-signer"));
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec {
            names: vec!["content_hash".to_string()],
        },
        crawl_token.to_string(),
    );
    let db = common::test_db_arc();
    let state = Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db)),
        chain: Arc::new(chain),
        node_pubkey_hex: signer.pubkey_hex().to_string(),
        signer,
        admin_token: "adr0084-sort-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    });

    for (hash, title, expected) in [
        ("hash-1", MESSY_TITLE, sort_key(MESSY_TITLE)),
        ("hash-2", "Changed  Title", "changed title".to_string()),
    ] {
        let url = "https://example.com/ingest-sort.xml";
        let payload = serde_json::json!({
            "canonical_url": url,
            "source_url": url,
            "crawl_token": crawl_token,
            "http_status": 200,
            "content_hash": hash,
            "feed_data": {
                "feed_guid": "ingest-sort-feed",
                "title": "Ingest Sort Feed",
                "raw_medium": "music",
                "explicit": false,
                "tracks": [{
                    "track_guid": "ingest-sort-track",
                    "title": title,
                    "explicit": false,
                }],
            },
        });
        let resp = stophammer::api::build_router(Arc::clone(&state))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/ingest/feed")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                    .expect("build request"),
            )
            .await
            .expect("send request");
        assert!(
            resp.status().is_success(),
            "ingest must succeed, got {}",
            resp.status()
        );

        let conn = db.lock().expect("db lock");
        assert_eq!(
            stored_sort(&conn, "ingest-sort-track"),
            Some(expected),
            "ADR 0084 §8: ingest_transaction must derive tracks.title_sort at each write. \
             Add title_sort to both INSERT statements in src/db.rs."
        );
    }
}
