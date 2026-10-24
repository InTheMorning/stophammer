// ADR 0073 §4: the track response gives both forms of a value time split.
//
// `include=value_time_splits` keeps each old field with its type, and adds
// the stated values. `include=local_value_time_splits` gives the local
// form, with its recipients.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0073-split-response-crawl-token";
const FEED_GUID: &str = "adr0073-response-feed";
const FEED_URL: &str = "https://a.example/adr0073-response.xml";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0073-split-response-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0073-split-response-admin-token".into(),
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
async fn ingest(st: &Arc<stophammer::api::AppState>, track_guid: &str, splits: Value, round: u32) {
    let payload = json!({
        "canonical_url": FEED_URL,
        "source_url": FEED_URL,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-adr0073-response-{round}"),
        "feed_data": {
            "feed_guid": FEED_GUID,
            "title": "ADR 0073 Response Feed",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Some Artist",
            "tracks": [{
                "track_guid": track_guid,
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

/// Reads one track of `FEED_GUID` with the given `include` value.
async fn get_track(st: &Arc<stophammer::api::AppState>, track_guid: &str, include: &str) -> Value {
    let uri = format!("/v1/feeds/{FEED_GUID}/tracks/{track_guid}?include={include}");
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
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
    assert!(
        status.is_success(),
        "GET track failed with {status}: {body}"
    );
    body["data"].clone()
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

#[tokio::test]
async fn a_remote_split_gives_the_old_fields_and_the_added_fields() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "track-remote-full",
        json!([{
            "form": "remote",
            "start_time_secs": 60.7,
            "duration_secs": 30,
            "remote_feed_guid": "remote-feed",
            "remote_percentage": 95
        }]),
        1,
    )
    .await;

    let data = get_track(&st, "track-remote-full", "value_time_splits").await;
    let rows = data["value_time_splits"].as_array().expect("an array");
    assert_eq!(rows.len(), 1, "one remote row: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["start_time_secs"], 60, "ADR 0073 §4: rounded down");
    assert_eq!(row["start_time"], 60.7, "ADR 0073 §4: the stated value");
    assert_eq!(row["split"], 95, "ADR 0073 §4: the effective percentage");
    assert_eq!(row["remote_percentage"], 95.0, "the stated percentage");
    assert_eq!(
        row["remote_item_guid"], "",
        "ADR 0073 §4: empty string when the source states none"
    );
    assert_eq!(row["remote_feed_guid"], "remote-feed");
}

#[tokio::test]
async fn a_remote_split_with_no_remote_percentage_gives_100_and_a_null_percentage() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "track-remote-no-pct",
        json!([{
            "form": "remote",
            "start_time_secs": 0,
            "duration_secs": 30,
            "remote_feed_guid": "remote-feed",
            "remote_item_guid": "remote-item"
        }]),
        1,
    )
    .await;

    let data = get_track(&st, "track-remote-no-pct", "value_time_splits").await;
    let row = &data["value_time_splits"].as_array().expect("an array")[0];
    assert_eq!(
        row["split"], 100,
        "ADR 0073 §4: 100 is the default effective percentage"
    );
    assert!(
        row["remote_percentage"].is_null(),
        "ADR 0073 §4: null when the source states none, got {row}"
    );
}

#[tokio::test]
async fn a_remote_percentage_over_100_clamps_the_split_but_keeps_the_stated_value() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "track-remote-over",
        json!([{
            "form": "remote",
            "start_time_secs": 0,
            "duration_secs": 30,
            "remote_feed_guid": "remote-feed",
            "remote_item_guid": "remote-item",
            "remote_percentage": 150
        }]),
        1,
    )
    .await;

    let data = get_track(&st, "track-remote-over", "value_time_splits").await;
    let row = &data["value_time_splits"].as_array().expect("an array")[0];
    assert_eq!(row["split"], 100, "ADR 0073 §4: the split limits to 100");
    assert_eq!(
        row["remote_percentage"], 150.0,
        "ADR 0073 §4: the added field keeps the stated value"
    );
}

#[tokio::test]
async fn a_remote_percentage_under_0_clamps_the_split_to_0() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "track-remote-under",
        json!([{
            "form": "remote",
            "start_time_secs": 0,
            "duration_secs": 30,
            "remote_feed_guid": "remote-feed",
            "remote_item_guid": "remote-item",
            "remote_percentage": -5
        }]),
        1,
    )
    .await;

    let data = get_track(&st, "track-remote-under", "value_time_splits").await;
    let row = &data["value_time_splits"].as_array().expect("an array")[0];
    assert_eq!(row["split"], 0, "ADR 0073 §4: the split limits to 0");
    assert_eq!(row["remote_percentage"], -5.0, "the stated value stays");
}

#[tokio::test]
async fn a_local_split_is_not_in_value_time_splits() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "track-local-only",
        json!([{
            "form": "local",
            "start_time_secs": 10,
            "duration_secs": 20,
            "recipients": [recipient("Alice", "02-alice", 100)]
        }]),
        1,
    )
    .await;

    let data = get_track(&st, "track-local-only", "value_time_splits").await;
    let rows = data["value_time_splits"].as_array().expect("an array");
    assert_eq!(
        rows.len(),
        0,
        "ADR 0073 §4: the list holds only the remote form: {rows:?}"
    );
}

#[tokio::test]
async fn local_value_time_splits_gives_the_local_split_with_its_recipients_in_order() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(
        &st,
        "track-local-order",
        json!([{
            "form": "local",
            "start_time_secs": 10.5,
            "duration_secs": 20,
            "recipients": [
                recipient("Alice", "02-alice", 85),
                recipient("Jimbob", "02-jimbob", 15),
            ]
        }]),
        1,
    )
    .await;

    let data = get_track(&st, "track-local-order", "local_value_time_splits").await;
    let rows = data["local_value_time_splits"]
        .as_array()
        .expect("an array");
    assert_eq!(rows.len(), 1, "one local row: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["position"], 0);
    assert_eq!(row["start_time"], 10.5);
    assert_eq!(row["duration"], 20.0);
    let recipients = row["recipients"].as_array().expect("recipients array");
    assert_eq!(recipients.len(), 2, "both recipients: {recipients:?}");
    assert_eq!(
        recipients[0]["recipient_name"], "Alice",
        "ADR 0073 §4: the recipients keep source order"
    );
    assert_eq!(recipients[0]["split"], 85);
    assert_eq!(recipients[1]["recipient_name"], "Jimbob");
    assert_eq!(recipients[1]["split"], 15);

    // A local split is still absent from the remote-form include.
    let remote_data = get_track(&st, "track-local-order", "value_time_splits").await;
    assert_eq!(
        remote_data["value_time_splits"]
            .as_array()
            .expect("an array")
            .len(),
        0
    );
}

#[tokio::test]
async fn a_track_with_no_split_gives_an_empty_list_for_each_include() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    ingest(&st, "track-no-split", json!([]), 1).await;

    let data = get_track(
        &st,
        "track-no-split",
        "value_time_splits,local_value_time_splits",
    )
    .await;
    assert_eq!(
        data["value_time_splits"]
            .as_array()
            .expect("an array")
            .len(),
        0,
        "ADR 0073 §4: no split gives an empty list, got {data}"
    );
    assert_eq!(
        data["local_value_time_splits"]
            .as_array()
            .expect("an array")
            .len(),
        0,
        "ADR 0073 §4: no split gives an empty list, got {data}"
    );
}
