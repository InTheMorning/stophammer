// ADR 0073 §2 and §3: the node stores, signs and applies both forms of a
// value time split, with the stated values.
//
// A remote split names a remote item. A local split names its recipients.
// The store keeps each stated value with no default and no limit. A node
// accepts the ingest of an old crawler, and applies an old event.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use stophammer::model::{RouteType, ValueTimeSplit, ValueTimeSplitForm};

const TOKEN: &str = "adr0073-split-store-crawl-token";
const FEED_GUID: &str = "adr0073-feed";
const FEED_URL: &str = "https://a.example/adr0073.xml";
const TRACK_GUID: &str = "adr0073-track";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0073-split-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0073-split-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

/// Sends one ingest with one track that has `splits`. `round` makes each
/// content hash different, so a second ingest is not a no-change.
async fn ingest(st: &Arc<stophammer::api::AppState>, splits: Value, round: u32) {
    let payload = json!({
        "canonical_url": FEED_URL,
        "source_url": FEED_URL,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-adr0073-{round}"),
        "feed_data": {
            "feed_guid": FEED_GUID,
            "title": "ADR 0073 Feed",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Some Artist",
            "tracks": [{
                "track_guid": TRACK_GUID,
                "title": "A Track",
                "pub_date": 1_700_000_000,
                "duration_secs": 400,
                "explicit": false,
                "payment_routes": [],
                "value_time_splits": splits,
            }],
        },
    });
    let req = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
        .expect("build request");
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(status.is_success(), "ingest failed with {status}: {body}");
    assert_eq!(
        body["accepted"], true,
        "the ingest must be accepted: {body}"
    );
}

fn splits_of(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<ValueTimeSplit> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_value_time_splits_for_feed_track(&conn, FEED_GUID, TRACK_GUID)
        .expect("read splits")
}

fn recipient_row_count(db: &Arc<Mutex<rusqlite::Connection>>) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT COUNT(*) FROM value_time_split_recipients",
        [],
        |r| r.get(0),
    )
    .expect("count recipient rows")
}

/// The two remote splits of the specification example.
fn remote_example() -> Value {
    json!([
        {
            "form": "remote",
            "start_time_secs": 60,
            "duration_secs": 237,
            "remote_feed_guid": "a94f5cc9-8c58-55fc-91fe-a324087a655b",
            "remote_item_guid": "https://podcastindex.org/podcast/4148683#1",
            "remote_percentage": 95
        },
        {
            "form": "remote",
            "start_time_secs": 330,
            "duration_secs": 53,
            "remote_feed_guid": "a94f5cc9-8c58-55fc-91fe-a324087a655b",
            "remote_item_guid": "https://podcastindex.org/podcast/4148683#3",
            "remote_start_time_secs": 174,
            "remote_percentage": 95
        }
    ])
}

fn recipient(name: &str, address: &str, split: i64) -> Value {
    json!({
        "recipient_name": name,
        "route_type": "node",
        "address": address,
        "custom_key": null,
        "custom_value": null,
        "split": split,
        "fee": false
    })
}

/// The two local splits of the specification example.
fn local_example() -> Value {
    json!([
        {
            "form": "local",
            "start_time_secs": 60,
            "duration_secs": 237,
            "recipients": [
                recipient("Alice (Podcaster)", "02-alice", 85),
                recipient("Jimbob (Guest)", "02-jimbob", 15),
            ]
        },
        {
            "form": "local",
            "start_time_secs": 330,
            "duration_secs": 53,
            "recipients": [
                recipient("Alice (Podcaster)", "02-alice", 50),
                recipient("Sara (Co-host)", "02-sara", 50),
            ]
        }
    ])
}

fn remote_split(start: f64, extra: &Value) -> Value {
    let mut split = json!({
        "form": "remote",
        "start_time_secs": start,
        "duration_secs": 30,
        "remote_feed_guid": "remote-feed",
        "remote_item_guid": "remote-item"
    });
    for (key, value) in extra.as_object().expect("extra is an object") {
        split[key] = value.clone();
    }
    split
}

#[tokio::test]
async fn the_remote_example_stores_two_rows_with_the_stated_values() {
    let db = common::test_db_arc();
    ingest(&state(Arc::clone(&db)), remote_example(), 1).await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 2, "two remote splits: {splits:?}");
    for (index, split) in splits.iter().enumerate() {
        assert_eq!(split.form, ValueTimeSplitForm::Remote, "a remote split");
        assert_eq!(
            split.position,
            i64::try_from(index).expect("index fits i64"),
            "the position is the source order"
        );
        assert_eq!(
            split.remote_percentage,
            Some(95.0),
            "ADR 0073 §2: the stated remote percentage is stored"
        );
        assert!(
            split.recipients.is_empty(),
            "a remote split has no recipient"
        );
    }
    assert_eq!(
        splits[0].remote_start_time_secs, None,
        "the first split states no remote start time"
    );
    assert_eq!(
        splits[1].remote_start_time_secs,
        Some(174.0),
        "ADR 0073 §2: the stated remote start time is stored"
    );
    assert_eq!(
        splits[1].remote_item_guid.as_deref(),
        Some("https://podcastindex.org/podcast/4148683#3")
    );
}

#[tokio::test]
async fn the_local_example_stores_two_local_rows_with_their_recipients_in_order() {
    let db = common::test_db_arc();
    ingest(&state(Arc::clone(&db)), local_example(), 1).await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 2, "two local splits: {splits:?}");
    let names: Vec<Vec<&str>> = splits
        .iter()
        .map(|s| {
            assert_eq!(s.form, ValueTimeSplitForm::Local, "a local split");
            assert_eq!(s.remote_feed_guid, None, "a local split has no remote feed");
            s.recipients
                .iter()
                .map(|r| r.recipient_name.as_deref().expect("recipient name"))
                .collect()
        })
        .collect();
    assert_eq!(
        names,
        vec![
            vec!["Alice (Podcaster)", "Jimbob (Guest)"],
            vec!["Alice (Podcaster)", "Sara (Co-host)"],
        ],
        "ADR 0073 §2: the recipients keep the source order"
    );
    assert_eq!(splits[0].recipients[0].split, 85);
    assert_eq!(splits[0].recipients[1].split, 15);
    assert_eq!(splits[0].recipients[0].route_type, RouteType::Node);
    assert_eq!(splits[0].recipients[0].address, "02-alice");
    assert!((splits[1].start_time_secs - 330.0).abs() < f64::EPSILON);
    assert_eq!(recipient_row_count(&db), 4, "four recipient rows");
}

#[tokio::test]
async fn a_fractional_start_time_is_stored() {
    let db = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db)),
        json!([remote_split(12.5, &json!({}))]),
        1,
    )
    .await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 1);
    assert!(
        (splits[0].start_time_secs - 12.5).abs() < f64::EPSILON,
        "ADR 0073 §2: the start time 12.5 is stored, got {}",
        splits[0].start_time_secs
    );
}

#[tokio::test]
async fn a_split_with_no_remote_percentage_stores_null() {
    let db = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db)),
        json!([remote_split(0.0, &json!({}))]),
        1,
    )
    .await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 1);
    assert_eq!(
        splits[0].remote_percentage, None,
        "ADR 0073 §2: the store applies no default of 100"
    );
}

#[tokio::test]
async fn a_remote_percentage_of_150_is_stored_as_150() {
    let db = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db)),
        json!([remote_split(0.0, &json!({ "remote_percentage": 150 }))]),
        1,
    )
    .await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 1);
    assert_eq!(
        splits[0].remote_percentage,
        Some(150.0),
        "ADR 0073 §2: the store applies no limit"
    );
}

#[tokio::test]
async fn a_remote_split_with_an_empty_remote_feed_guid_is_dropped_and_the_other_stays() {
    let db = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db)),
        json!([
            remote_split(0.0, &json!({ "remote_feed_guid": "" })),
            remote_split(60.0, &json!({})),
        ]),
        1,
    )
    .await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 1, "only the valid split stays: {splits:?}");
    assert!((splits[0].start_time_secs - 60.0).abs() < f64::EPSILON);
    assert_eq!(
        splits[0].position, 1,
        "the position is the index of the split in the ingest list"
    );
}

#[tokio::test]
async fn a_remote_split_with_a_recipient_is_dropped_and_the_other_stays() {
    let db = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db)),
        json!([
            remote_split(
                0.0,
                &json!({ "recipients": [recipient("Mixed", "02-mixed", 100)] }),
            ),
            remote_split(60.0, &json!({})),
        ]),
        1,
    )
    .await;

    let splits = splits_of(&db);
    assert_eq!(
        splits.len(),
        1,
        "ADR 0073 §1: a split with children of the two forms is dropped: {splits:?}"
    );
    assert_eq!(splits[0].form, ValueTimeSplitForm::Remote);
    assert!((splits[0].start_time_secs - 60.0).abs() < f64::EPSILON);
    assert_eq!(recipient_row_count(&db), 0, "no recipient row is stored");
}

#[tokio::test]
async fn a_local_split_with_a_remote_feed_guid_is_dropped_and_the_other_stays() {
    let mut mixed = local_example()[0].clone();
    mixed["remote_feed_guid"] = json!("remote-feed");
    let valid = local_example()[1].clone();
    let db = common::test_db_arc();
    ingest(&state(Arc::clone(&db)), json!([mixed, valid]), 1).await;

    let splits = splits_of(&db);
    assert_eq!(
        splits.len(),
        1,
        "ADR 0073 §1: a split with children of the two forms is dropped: {splits:?}"
    );
    assert_eq!(splits[0].form, ValueTimeSplitForm::Local);
    assert_eq!(splits[0].position, 1, "the valid second split stays");
    assert_eq!(
        recipient_row_count(&db),
        2,
        "only the recipients of the valid split are stored"
    );
}

#[tokio::test]
async fn an_ingest_in_the_old_shape_stores_a_remote_row() {
    let db = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db)),
        json!([{
            "start_time_secs": 60,
            "duration_secs": 30,
            "remote_feed_guid": "old-remote-feed",
            "remote_item_guid": "old-remote-item",
            "split": 50
        }]),
        1,
    )
    .await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 1, "the old-shape split is stored");
    assert_eq!(splits[0].form, ValueTimeSplitForm::Remote);
    assert!((splits[0].start_time_secs - 60.0).abs() < f64::EPSILON);
    assert_eq!(
        splits[0].remote_feed_guid.as_deref(),
        Some("old-remote-feed")
    );
    assert_eq!(splits[0].remote_percentage, None);
}

fn events_of(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<stophammer::event::Event> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
}

fn apply_all(events: &[stophammer::event::Event]) -> Arc<Mutex<rusqlite::Connection>> {
    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in events {
        let result = stophammer::apply::apply_single_event(&pool_b, ev);
        assert!(
            result.is_ok(),
            "event {:?} must apply: {result:?}",
            ev.event_type
        );
    }
    db_b
}

/// Removes the fields that differ between two nodes: the row ids and the
/// write time.
fn comparable(splits: Vec<ValueTimeSplit>) -> Vec<ValueTimeSplit> {
    splits
        .into_iter()
        .map(|mut s| {
            s.id = None;
            s.created_at = 0;
            s
        })
        .collect()
}

#[tokio::test]
async fn the_signed_event_carries_both_forms_and_a_second_node_gives_the_same_rows() {
    let db_a = common::test_db_arc();
    let mut both = remote_example().as_array().expect("array").clone();
    both.extend(local_example().as_array().expect("array").iter().cloned());
    ingest(&state(Arc::clone(&db_a)), Value::Array(both), 1).await;

    let events = events_of(&db_a);
    let track_event = events
        .iter()
        .find_map(|ev| match &ev.payload {
            stophammer::event::EventPayload::TrackUpserted(p)
                if p.track.track_guid == TRACK_GUID =>
            {
                Some(p)
            }
            _ => None,
        })
        .expect("the ingest signs a TrackUpserted event");
    let forms: Vec<ValueTimeSplitForm> = track_event
        .value_time_splits
        .iter()
        .map(|s| s.form)
        .collect();
    assert_eq!(
        forms,
        vec![
            ValueTimeSplitForm::Remote,
            ValueTimeSplitForm::Remote,
            ValueTimeSplitForm::Local,
            ValueTimeSplitForm::Local,
        ],
        "ADR 0073 §3: the event carries both forms"
    );

    let db_b = apply_all(&events);
    let splits_a = comparable(splits_of(&db_a));
    let splits_b = comparable(splits_of(&db_b));
    assert_eq!(splits_a.len(), 4);
    assert_eq!(
        splits_a, splits_b,
        "ADR 0073 §3: a second node gives the same rows and recipients"
    );
    assert_eq!(recipient_row_count(&db_b), 4);
}

#[tokio::test]
async fn an_event_in_the_old_shape_applies() {
    let db_a = common::test_db_arc();
    ingest(
        &state(Arc::clone(&db_a)),
        json!([remote_split(0.0, &json!({}))]),
        1,
    )
    .await;

    let mut events = events_of(&db_a);
    let mut rewritten = 0;
    for ev in &mut events {
        if matches!(
            ev.payload,
            stophammer::event::EventPayload::TrackUpserted(_)
        ) {
            // Apply reads the signed `payload_json`, so the old form goes
            // there: an integer time, a `split`, and no field of ADR 0073.
            let mut payload: Value = serde_json::from_str(&ev.payload_json).expect("parse payload");
            payload["value_time_splits"] = json!([{
                "id": null,
                "source_feed_guid": FEED_GUID,
                "source_track_guid": TRACK_GUID,
                "start_time_secs": 60,
                "duration_secs": 30,
                "remote_feed_guid": "old-remote-feed",
                "remote_item_guid": "old-remote-item",
                "split": 50,
                "created_at": 1
            }]);
            ev.payload_json = payload.to_string();
            rewritten += 1;
        }
    }
    assert_eq!(rewritten, 1, "one TrackUpserted event");

    let db_b = apply_all(&events);
    let splits = splits_of(&db_b);
    assert_eq!(splits.len(), 1, "the old event writes its split");
    assert_eq!(splits[0].form, ValueTimeSplitForm::Remote);
    assert!((splits[0].start_time_secs - 60.0).abs() < f64::EPSILON);
    assert_eq!(splits[0].duration_secs, Some(30.0));
    assert_eq!(
        splits[0].remote_feed_guid.as_deref(),
        Some("old-remote-feed")
    );
    assert_eq!(splits[0].remote_percentage, None);
}

#[tokio::test]
async fn a_second_ingest_that_removes_a_local_split_deletes_its_recipients() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, local_example(), 1).await;
    assert_eq!(recipient_row_count(&db), 4);

    let first_only = json!([local_example()[0].clone()]);
    ingest(&st, first_only, 2).await;

    let splits = splits_of(&db);
    assert_eq!(splits.len(), 1, "one local split stays");
    assert_eq!(
        recipient_row_count(&db),
        2,
        "the recipients of the removed split are deleted"
    );
}

#[tokio::test]
async fn the_removal_of_a_feed_leaves_no_recipient_row() {
    let db = common::test_db_arc();
    ingest(&state(Arc::clone(&db)), local_example(), 1).await;
    assert_eq!(recipient_row_count(&db), 4);

    {
        let mut conn = db.lock().expect("lock db");
        stophammer::db::delete_feed(&mut conn, FEED_GUID).expect("delete feed");
    }
    assert_eq!(
        recipient_row_count(&db),
        0,
        "the removal of a feed leaves no row in value_time_split_recipients"
    );
}

#[tokio::test]
async fn the_track_trigger_deletes_the_recipients_with_foreign_keys_off() {
    let db = common::test_db_arc();
    ingest(&state(Arc::clone(&db)), local_example(), 1).await;
    assert_eq!(recipient_row_count(&db), 4);

    {
        let conn = db.lock().expect("lock db");
        conn.execute_batch("PRAGMA foreign_keys = OFF;")
            .expect("turn foreign keys off");
        conn.execute(
            "DELETE FROM tracks WHERE feed_guid = ?1 AND track_guid = ?2",
            rusqlite::params![FEED_GUID, TRACK_GUID],
        )
        .expect("delete the track");
    }
    assert_eq!(
        recipient_row_count(&db),
        0,
        "trg_tracks_cleanup_before_delete must delete the recipients, with no cascade"
    );
}
