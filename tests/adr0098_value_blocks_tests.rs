//! ADR 0098 §4: `payment_routes` gives only the Lightning routes that a
//! client can pay, and `value_blocks` gives each stored block.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0098-value-blocks-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("adr0098-value-blocks"));
    let pubkey = signer.pubkey_hex().to_string();
    let chain = stophammer::verify::build_chain(
        &stophammer::verify::ChainSpec { names: vec![] },
        TOKEN.to_string(),
    );
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "admin".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: vec![],
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn send(st: &Arc<stophammer::api::AppState>, method: &str, uri: &str, body: Value) -> Value {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).expect("encode body")))
        .expect("build request");
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req)
        .await
        .expect("send request");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    let value: Value = serde_json::from_slice(&bytes).expect("decode body");
    assert!(status.is_success(), "request failed: {status} {value}");
    value
}

const NODE_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef01";
const USDC_ADDRESS: &str = "eip155:8453:0x1111111111111111111111111111111111111111";

/// One recipient with the fields of its block.
fn route(address: &str, route_type: &str, block: Option<(i64, &str, &str)>) -> Value {
    let mut value = json!({
        "recipient_name": address,
        "route_type": route_type,
        "address": address,
        "split": 100,
        "fee": false,
    });
    if let Some((block_index, value_type, value_method)) = block {
        value["block_index"] = json!(block_index);
        value["value_type"] = json!(value_type);
        value["value_method"] = json!(value_method);
    }
    value
}

const LIGHTNING: Option<(i64, &str, &str)> = Some((0, "lightning", "keysend"));
const USDC: Option<(i64, &str, &str)> = Some((1, "usdc", "eip3009"));

fn two_blocks() -> Value {
    json!([
        route(NODE_KEY, "node", LIGHTNING),
        route(USDC_ADDRESS, "address", USDC)
    ])
}

fn track(guid: &str, routes: Value, splits: Value) -> Value {
    let mut value = json!({
        "track_guid": guid,
        "title": guid,
        "pub_date": 1_700_000_000,
        "duration_secs": 100,
        "explicit": false,
    });
    value["payment_routes"] = routes;
    value["value_time_splits"] = splits;
    value
}

async fn ingest(st: &Arc<stophammer::api::AppState>, feed_routes: Value, tracks: Value) {
    let url = "https://value.example/feed.xml";
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": "value-blocks-hash",
        "feed_data": {
            "feed_guid": "value-blocks-feed",
            "title": "Feed",
            "raw_medium": "music",
            "explicit": false,
            "author_name": "Artist",
            "feed_payment_routes": feed_routes,
            "tracks": tracks,
        },
    });
    let body = send(st, "POST", "/ingest/feed", payload).await;
    assert_eq!(body["accepted"], true, "ingest must be accepted: {body}");
}

async fn feed_data(st: &Arc<stophammer::api::AppState>, include: &str) -> Value {
    let uri = format!("/v1/feeds/value-blocks-feed?include={include}");
    send(st, "GET", &uri, json!({})).await["data"].clone()
}

async fn track_data(st: &Arc<stophammer::api::AppState>, guid: &str, include: &str) -> Value {
    let uri = format!("/v1/feeds/value-blocks-feed/tracks/{guid}?include={include}");
    send(st, "GET", &uri, json!({})).await["data"].clone()
}

fn addresses(routes: &Value) -> Vec<&str> {
    routes
        .as_array()
        .expect("a list of routes")
        .iter()
        .map(|route| route["address"].as_str().expect("an address"))
        .collect()
}

/// Asserts criterion 1 and criterion 2 on the data of a feed or of a track.
fn assert_two_blocks(data: &Value) {
    assert_eq!(addresses(&data["payment_routes"]), vec![NODE_KEY]);
    let blocks = data["value_blocks"].as_array().expect("a list of blocks");
    assert_eq!(blocks.len(), 2, "two blocks: {blocks:?}");
    assert_eq!(blocks[0]["block_index"], 0);
    assert_eq!(blocks[0]["value_type"], "lightning");
    assert_eq!(blocks[0]["value_method"], "keysend");
    assert_eq!(addresses(&blocks[0]["recipients"]), vec![NODE_KEY]);
    assert_eq!(blocks[0]["recipients"][0]["route_type"], "node");
    assert_eq!(blocks[1]["block_index"], 1);
    assert_eq!(blocks[1]["value_type"], "usdc");
    assert_eq!(blocks[1]["value_method"], "eip3009");
    assert_eq!(addresses(&blocks[1]["recipients"]), vec![USDC_ADDRESS]);
    assert_eq!(blocks[1]["recipients"][0]["route_type"], "address");
    assert_eq!(blocks[1]["recipients"][0]["split"], 100);
    assert_eq!(blocks[1]["recipients"][0]["fee"], false);
}

// Criterion 1.
#[tokio::test]
async fn feed_gives_lightning_routes_and_each_block() {
    let st = state(common::test_db_arc());
    ingest(&st, two_blocks(), json!([track("t", json!([]), json!([]))])).await;
    let data = feed_data(&st, "payment_routes,value_blocks").await;
    assert_two_blocks(&data);
}

// Criterion 2.
#[tokio::test]
async fn track_gives_lightning_routes_and_each_block() {
    let st = state(common::test_db_arc());
    ingest(&st, json!([]), json!([track("t", two_blocks(), json!([]))])).await;
    let data = track_data(&st, "t", "payment_routes,value_blocks").await;
    assert_two_blocks(&data);
}

// Criterion 3, and the capital letters of a public key.
#[tokio::test]
async fn payment_routes_drops_unknown_type_and_bad_node_address() {
    let st = state(common::test_db_arc());
    let upper = NODE_KEY.to_uppercase();
    let not_hex = format!("zz{}", &NODE_KEY[2..]);
    let routes = json!([
        route(NODE_KEY, "node", LIGHTNING),
        route(&upper, "node", LIGHTNING),
        route("artist@example.com", "lnaddress", LIGHTNING),
        route("some-wallet", "lightning", LIGHTNING),
        route("0xabc", "node", LIGHTNING),
        route(&not_hex, "node", LIGHTNING),
        route(NODE_KEY, "keysend", LIGHTNING),
    ]);
    ingest(&st, routes.clone(), json!([track("t", routes, json!([]))])).await;
    for data in [
        feed_data(&st, "payment_routes,value_blocks").await,
        track_data(&st, "t", "payment_routes,value_blocks").await,
    ] {
        assert_eq!(
            addresses(&data["payment_routes"]),
            vec![NODE_KEY, upper.as_str(), "artist@example.com"]
        );
        let blocks = data["value_blocks"].as_array().expect("a list of blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            addresses(&blocks[0]["recipients"]),
            vec![
                NODE_KEY,
                upper.as_str(),
                "artist@example.com",
                "some-wallet",
                "0xabc",
                not_hex.as_str(),
                NODE_KEY
            ]
        );
        assert_eq!(blocks[0]["recipients"][3]["route_type"], "lightning");
        assert_eq!(blocks[0]["recipients"][4]["route_type"], "node");
    }
}

// Criterion 4.
#[tokio::test]
async fn rows_with_null_block_columns_are_one_first_block() {
    let st = state(common::test_db_arc());
    let old_rows = json!([route(NODE_KEY, "node", None), route("0xabc", "node", None)]);
    let mixed = json!([
        route(USDC_ADDRESS, "address", USDC),
        route(NODE_KEY, "node", None)
    ]);
    ingest(&st, old_rows, json!([track("t", mixed, json!([]))])).await;

    let feed = feed_data(&st, "payment_routes,value_blocks").await;
    assert_eq!(addresses(&feed["payment_routes"]), vec![NODE_KEY]);
    let blocks = feed["value_blocks"].as_array().expect("a list of blocks");
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0]["block_index"].is_null());
    assert!(blocks[0]["value_type"].is_null());
    assert!(blocks[0]["value_method"].is_null());
    assert_eq!(addresses(&blocks[0]["recipients"]), vec![NODE_KEY, "0xabc"]);

    // The block with no index is the first one.
    let track = track_data(&st, "t", "payment_routes,value_blocks").await;
    assert_eq!(addresses(&track["payment_routes"]), vec![NODE_KEY]);
    let blocks = track["value_blocks"].as_array().expect("a list of blocks");
    assert_eq!(blocks.len(), 2);
    assert!(blocks[0]["block_index"].is_null());
    assert_eq!(blocks[1]["block_index"], 1);
}

// Criterion 5.
#[tokio::test]
async fn track_with_no_stored_route_gets_the_routes_of_its_feed() {
    let st = state(common::test_db_arc());
    let only_usdc = json!([route(USDC_ADDRESS, "address", Some((0, "usdc", "eip3009")))]);
    let tracks = json!([
        track("no-route", json!([]), json!([])),
        track("only-usdc", only_usdc, json!([])),
    ]);
    ingest(&st, two_blocks(), tracks).await;

    let data = track_data(&st, "no-route", "payment_routes,value_blocks").await;
    assert_two_blocks(&data);

    let data = track_data(&st, "only-usdc", "payment_routes,value_blocks").await;
    assert_eq!(addresses(&data["payment_routes"]), Vec::<&str>::new());
    let blocks = data["value_blocks"].as_array().expect("a list of blocks");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["value_type"], "usdc");
}

// Criterion 6.
#[tokio::test]
async fn split_includes_give_the_block_fields() {
    let st = state(common::test_db_arc());
    let splits = json!([
        {
            "start_time_secs": 0,
            "duration_secs": 10,
            "remote_feed_guid": "remote-feed",
            "remote_item_guid": "remote-item",
            "value_type": "usdc",
            "value_method": "eip3009",
            "block_index": 1,
        },
        {
            "form": "local",
            "start_time_secs": 10,
            "duration_secs": 10,
            "value_type": "usdc",
            "value_method": "eip3009",
            "block_index": 1,
            "recipients": [route(USDC_ADDRESS, "address", USDC)],
        },
        {
            "start_time_secs": 20,
            "duration_secs": 10,
            "remote_feed_guid": "remote-feed",
            "remote_item_guid": "old-item",
        },
    ]);
    ingest(&st, json!([]), json!([track("t", two_blocks(), splits)])).await;
    let data = track_data(&st, "t", "value_time_splits,local_value_time_splits").await;

    let remote = data["value_time_splits"].as_array().expect("remote splits");
    assert_eq!(remote.len(), 2);
    assert_eq!(remote[0]["block_index"], 1);
    assert_eq!(remote[0]["value_type"], "usdc");
    assert_eq!(remote[0]["value_method"], "eip3009");
    // A split with no block field gives null.
    assert!(remote[1]["block_index"].is_null());
    assert!(remote[1]["value_type"].is_null());
    assert!(remote[1]["value_method"].is_null());

    let local = data["local_value_time_splits"]
        .as_array()
        .expect("local splits");
    assert_eq!(local.len(), 1);
    assert_eq!(local[0]["block_index"], 1);
    assert_eq!(local[0]["value_type"], "usdc");
    assert_eq!(local[0]["value_method"], "eip3009");
    assert_eq!(local[0]["recipients"][0]["route_type"], "address");
}

// A response with no `include=value_blocks` has no such key.
#[tokio::test]
async fn value_blocks_key_is_absent_with_no_include() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        two_blocks(),
        json!([track("t", two_blocks(), json!([]))]),
    )
    .await;
    let feed = feed_data(&st, "payment_routes").await;
    assert!(feed.get("value_blocks").is_none());
    let track = track_data(&st, "t", "payment_routes").await;
    assert!(track.get("value_blocks").is_none());
}

// Criterion 7.
#[tokio::test]
async fn capabilities_and_openapi_declare_value_blocks() {
    let st = state(common::test_db_arc());
    let capabilities = send(&st, "GET", "/v1/node/capabilities", json!({})).await;
    assert!(
        capabilities["include_params"]["feed"]
            .as_array()
            .expect("feed includes")
            .contains(&json!("value_blocks"))
    );
    assert!(
        capabilities["include_params"]["track"]
            .as_array()
            .expect("track includes")
            .contains(&json!("value_blocks"))
    );
    let document =
        serde_json::to_value(stophammer::openapi::primary_document()).expect("OpenAPI document");
    let schemas = &document["components"]["schemas"];
    for schema in ["FeedResponse", "TrackResponse"] {
        assert!(
            !schemas[schema]["properties"]["value_blocks"].is_null(),
            "the OpenAPI document declares value_blocks of {schema}"
        );
    }
    assert!(
        !schemas["ValueBlockResponse"]["properties"]["recipients"].is_null(),
        "the OpenAPI document declares the block schema"
    );
    let text = document.to_string();
    assert!(
        text.contains("value_blocks"),
        "the OpenAPI document names the include"
    );
}
