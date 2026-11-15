mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use stophammer::event::{Event, EventPayload, EventType};
use stophammer::model::FeedListValueRaw;

const TOKEN: &str = "adr0098-block-fields-crawl-token";
const FEED_GUID: &str = "adr0098-feed";
const TRACK_GUID: &str = "adr0098-track";
const FEED_URL: &str = "https://a.example/adr0098.xml";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0098-block-fields"));
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
        admin_token: "test-adr0098-admin".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    feed_routes: Value,
    tracks: Value,
    round: u32,
    medium: &str,
) {
    let payload = json!({
        "canonical_url": FEED_URL,
        "source_url": FEED_URL,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-adr0098-{round}"),
        "feed_data": {
            "feed_guid": FEED_GUID,
            "title": "ADR 0098 Feed",
            "raw_medium": medium,
            "explicit": false,
            "author_name": "Some Artist",
            "feed_payment_routes": feed_routes,
            "tracks": tracks,
        },
    });
    let request = Request::builder()
        .method("POST")
        .uri("/ingest/feed")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize request"),
        ))
        .expect("build request");
    let response = stophammer::api::build_router(Arc::clone(st))
        .oneshot(request)
        .await
        .expect("send request");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("read response")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("parse response");
    assert!(status.is_success(), "ingest failed with {status}: {body}");
    assert_eq!(body["accepted"], true, "ingest must be accepted: {body}");
}

fn track(mut routes: Value, mut splits: Value) -> Value {
    json!([{
        "track_guid": TRACK_GUID,
        "title": "A Track",
        "pub_date": 1_700_000_000,
        "duration_secs": 400,
        "explicit": false,
        "payment_routes": std::mem::take(&mut routes),
        "value_time_splits": std::mem::take(&mut splits),
    }])
}

fn route(address: &str, split: i64, block_index: i64, value_type: &str) -> Value {
    json!({
        "recipient_name": address,
        "route_type": "address",
        "address": address,
        "split": split,
        "fee": false,
        "value_type": value_type,
        "value_method": "keysend",
        "block_index": block_index,
    })
}

fn events(db: &Arc<Mutex<rusqlite::Connection>>) -> Vec<Event> {
    stophammer::db::get_events_since(&db.lock().expect("lock db"), 0, 10_000)
        .expect("read signed events")
}

fn count_type(events: &[Event], event_type: &EventType, subject: &str) -> usize {
    events
        .iter()
        .filter(|event| &event.event_type == event_type && event.subject_guid == subject)
        .count()
}

#[tokio::test]
async fn ingest_stores_block_fields_for_all_route_and_split_records() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let feed_routes = json!([route("feed-one", 100, 0, "lightning")]);
    let routes = json!([
        route("track-one", 100, 0, "lightning"),
        route("track-two", 100, 1, "usdc")
    ]);
    let splits = json!([{
        "form": "local",
        "start_time_secs": 0,
        "duration_secs": 10,
        "value_type": "usdc",
        "value_method": "base",
        "block_index": 3,
        "recipients": [route("local-one", 100, 3, "usdc")]
    }]);
    ingest(&st, feed_routes.clone(), track(routes, splits), 1, "music").await;

    {
        let conn = db.lock().expect("lock db");
        let track_routes =
            stophammer::db::get_payment_routes_for_feed_track(&conn, FEED_GUID, TRACK_GUID)
                .expect("read track routes");
        assert_eq!(track_routes.len(), 2);
        assert_eq!(track_routes[0].value_type.as_deref(), Some("lightning"));
        assert_eq!(track_routes[0].value_method.as_deref(), Some("keysend"));
        assert_eq!(track_routes[0].block_index, Some(0));
        assert_eq!(track_routes[1].value_type.as_deref(), Some("usdc"));
        assert_eq!(track_routes[1].block_index, Some(1));

        let feed_route_rows = stophammer::db::get_feed_payment_routes_for_feed(&conn, FEED_GUID)
            .expect("read feed routes");
        assert_eq!(feed_route_rows[0].value_type.as_deref(), Some("lightning"));
        assert_eq!(feed_route_rows[0].value_method.as_deref(), Some("keysend"));
        assert_eq!(feed_route_rows[0].block_index, Some(0));

        let splits =
            stophammer::db::get_value_time_splits_for_feed_track(&conn, FEED_GUID, TRACK_GUID)
                .expect("read value time splits");
        assert_eq!(splits[0].value_type.as_deref(), Some("usdc"));
        assert_eq!(splits[0].value_method.as_deref(), Some("base"));
        assert_eq!(splits[0].block_index, Some(3));
        assert_eq!(splits[0].recipients[0].value_type.as_deref(), Some("usdc"));
        assert_eq!(
            splits[0].recipients[0].value_method.as_deref(),
            Some("keysend")
        );
        assert_eq!(splits[0].recipients[0].block_index, Some(3));
    }

    let db_list = common::test_db_arc();
    let st_list = state(Arc::clone(&db_list));
    ingest(
        &st_list,
        feed_routes,
        track(json!([]), json!([])),
        2,
        "musicL",
    )
    .await;
    let conn = db_list.lock().expect("lock list DB");
    let list_routes: Vec<FeedListValueRaw> =
        stophammer::db::get_feed_list_value_raw_for_feed(&conn, FEED_GUID)
            .expect("read list value routes");
    assert_eq!(list_routes.len(), 1);
    assert_eq!(list_routes[0].value_type.as_deref(), Some("lightning"));
    assert_eq!(list_routes[0].value_method.as_deref(), Some("keysend"));
    assert_eq!(list_routes[0].block_index, Some(0));
}

#[tokio::test]
async fn omitted_block_fields_store_null() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let simple = json!([{ "route_type": "node", "address": "old", "split": 100, "fee": false }]);
    let split = json!([{
        "form": "local",
        "start_time_secs": 0,
        "recipients": [{ "route_type": "node", "address": "local", "split": 100, "fee": false }]
    }]);
    ingest(&st, simple.clone(), track(simple, split), 1, "music").await;
    let conn = db.lock().expect("lock db");
    for table in [
        "payment_routes",
        "feed_payment_routes",
        "value_time_splits",
        "value_time_split_recipients",
    ] {
        let null_count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE value_type IS NULL AND value_method IS NULL AND block_index IS NULL"),
            [],
            |row| row.get(0),
        ).expect("count null block metadata");
        assert_eq!(null_count, 1, "{table} stores null block metadata");
    }
}

#[tokio::test]
async fn omitted_list_block_fields_store_null() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let routes = json!([{
        "route_type": "node",
        "address": "legacy-list-address",
        "split": 100,
        "fee": false
    }]);
    ingest(&st, routes, track(json!([]), json!([])), 1, "musicL").await;
    let conn = db.lock().expect("lock list DB");
    let fields: (Option<String>, Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT value_type, value_method, block_index FROM feed_list_value_raw WHERE feed_guid = ?1",
            [FEED_GUID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read list block metadata");
    assert_eq!(fields, (None, None, None));
}

#[tokio::test]
async fn address_route_type_and_metadata_survive_signed_event_apply() {
    let db_a = common::test_db_arc();
    let st = state(Arc::clone(&db_a));
    ingest(
        &st,
        json!([]),
        track(json!([route("0xabc", 100, 4, "usdc")]), json!([])),
        1,
        "music",
    )
    .await;
    let events = events(&db_a);
    let payload = events
        .iter()
        .find_map(|event| match &event.payload {
            EventPayload::TrackUpserted(payload) if payload.track.track_guid == TRACK_GUID => {
                Some(payload)
            }
            _ => None,
        })
        .expect("signed TrackUpserted event exists");
    assert_eq!(payload.routes[0].route_type.as_str(), "address");
    assert_eq!(payload.routes[0].block_index, Some(4));

    let db_b = common::test_db_arc();
    let pool = common::wrap_pool(Arc::clone(&db_b));
    for event in &events {
        stophammer::apply::apply_single_event(&pool, event).expect("apply signed event");
    }
    let conn = db_b.lock().expect("lock replica DB");
    let routes = stophammer::db::get_payment_routes_for_feed_track(&conn, FEED_GUID, TRACK_GUID)
        .expect("read replica routes");
    assert_eq!(routes[0].route_type.as_str(), "address");
    assert_eq!(routes[0].value_type.as_deref(), Some("usdc"));
    assert_eq!(routes[0].value_method.as_deref(), Some("keysend"));
    assert_eq!(routes[0].block_index, Some(4));
}

#[tokio::test]
async fn old_track_payload_applies_with_null_block_fields() {
    let db_a = common::test_db_arc();
    let st = state(Arc::clone(&db_a));
    ingest(
        &st,
        json!([]),
        track(json!([route("old", 100, 0, "lightning")]), json!([])),
        1,
        "music",
    )
    .await;
    let mut events = events(&db_a);
    for event in &mut events {
        if event.event_type == EventType::TrackUpserted {
            let mut payload: Value =
                serde_json::from_str(&event.payload_json).expect("parse payload");
            for route in payload["routes"].as_array_mut().expect("route array") {
                route
                    .as_object_mut()
                    .expect("route object")
                    .remove("value_type");
                route
                    .as_object_mut()
                    .expect("route object")
                    .remove("value_method");
                route
                    .as_object_mut()
                    .expect("route object")
                    .remove("block_index");
            }
            event.payload_json = payload.to_string();
        }
    }
    let db_b = common::test_db_arc();
    let pool = common::wrap_pool(Arc::clone(&db_b));
    for event in &events {
        stophammer::apply::apply_single_event(&pool, event).expect("apply old payload event");
    }
    let conn = db_b.lock().expect("lock replica DB");
    let fields: (Option<String>, Option<String>, Option<i64>) = conn.query_row(
        "SELECT value_type, value_method, block_index FROM payment_routes WHERE track_guid = ?1",
        [TRACK_GUID],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).expect("read old payload route");
    assert_eq!(fields, (None, None, None));
}

#[tokio::test]
async fn route_only_change_emits_track_event_and_no_change_does_not() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let first = track(json!([route("old", 100, 0, "lightning")]), json!([]));
    ingest(&st, json!([]), first.clone(), 1, "music").await;
    ingest(
        &st,
        json!([]),
        track(json!([route("new", 100, 0, "lightning")]), json!([])),
        2,
        "music",
    )
    .await;
    let after_change = events(&db);
    assert_eq!(
        count_type(&after_change, &EventType::TrackUpserted, TRACK_GUID),
        2,
        "route-only change signs a TrackUpserted event"
    );
    ingest(
        &st,
        json!([]),
        track(json!([route("new", 100, 0, "lightning")]), json!([])),
        3,
        "music",
    )
    .await;
    assert_eq!(
        count_type(&events(&db), &EventType::TrackUpserted, TRACK_GUID),
        2,
        "unchanged track payment data signs no TrackUpserted event"
    );
    let split_only = json!([{
        "form": "local",
        "start_time_secs": 20,
        "duration_secs": 10,
        "recipients": [route("split-only", 100, 0, "lightning")]
    }]);
    ingest(
        &st,
        json!([]),
        track(json!([route("new", 100, 0, "lightning")]), split_only),
        4,
        "music",
    )
    .await;
    assert_eq!(
        count_type(&events(&db), &EventType::TrackUpserted, TRACK_GUID),
        3,
        "split-only change signs a TrackUpserted event"
    );
}

#[tokio::test]
async fn feed_route_value_type_change_emits_feed_routes_event() {
    let db = common::test_db_arc();
    let st = state(Arc::clone(&db));
    let track_data = track(json!([]), json!([]));
    ingest(
        &st,
        json!([route("feed", 100, 0, "lightning")]),
        track_data.clone(),
        1,
        "music",
    )
    .await;
    ingest(
        &st,
        json!([route("feed", 100, 0, "usdc")]),
        track_data,
        2,
        "music",
    )
    .await;
    let events = events(&db);
    assert_eq!(
        count_type(&events, &EventType::FeedRoutesReplaced, FEED_GUID),
        2
    );
    let changed = events.iter().find_map(|event| match &event.payload {
        EventPayload::FeedRoutesReplaced(payload)
            if payload
                .routes
                .first()
                .and_then(|route| route.value_type.as_deref())
                == Some("usdc") =>
        {
            Some(payload)
        }
        _ => None,
    });
    assert!(
        changed.is_some(),
        "event has the changed feed route value type"
    );
}

#[test]
fn payment_route_sum_groups_routes_by_block_index() {
    use stophammer::ingest::{
        IngestFeedData, IngestFeedRequest, IngestPaymentRoute, IngestTrackData,
    };
    use stophammer::model::RouteType;
    use stophammer::verify::{IngestContext, Verifier, VerifyResult};

    fn check(splits: &[(Option<i64>, i64)]) -> VerifyResult {
        let conn = common::test_db();
        let mut routes = Vec::new();
        for &(block_index, split) in splits {
            routes.push(IngestPaymentRoute {
                recipient_name: None,
                route_type: RouteType::from_text("address"),
                address: format!("address-{block_index:?}-{split}"),
                custom_key: None,
                custom_value: None,
                split,
                fee: false,
                value_type: Some("usdc".into()),
                value_method: Some("keysend".into()),
                block_index,
            });
        }
        let request = IngestFeedRequest {
            crawl_token: String::new(),
            canonical_url: String::new(),
            source_url: String::new(),
            http_status: 200,
            content_hash: String::new(),
            force_reingest: false,
            redirects: vec![],
            feed_data: Some(IngestFeedData {
                feed_guid: FEED_GUID.into(),
                title: "Feed".into(),
                description: None,
                image_url: None,
                language: None,
                explicit: false,
                itunes_type: None,
                raw_medium: None,
                last_build_date: None,
                new_feed_url: None,
                locked: None,
                locked_owner: None,
                author_name: None,
                owner_name: None,
                pub_date: None,
                remote_items: vec![],
                persons: vec![],
                entity_ids: vec![],
                links: vec![],
                feed_payment_routes: vec![],
                live_items: vec![],
                blocks: vec![],
                tracks: vec![IngestTrackData {
                    track_guid: TRACK_GUID.into(),
                    title: "Track".into(),
                    pub_date: None,
                    duration_secs: None,
                    enclosure_url: None,
                    enclosure_type: None,
                    enclosure_bytes: None,
                    alternate_enclosures: vec![],
                    track_number: None,
                    season: None,
                    explicit: false,
                    description: None,
                    author_name: None,
                    remote_items: vec![],
                    image_url: None,
                    language: None,
                    persons: vec![],
                    entity_ids: vec![],
                    links: vec![],
                    payment_routes: routes,
                    value_time_splits: vec![],
                    transcripts: vec![],
                }],
            }),
        };
        let ctx = IngestContext {
            request: &request,
            db: &conn,
            existing: None,
        };
        stophammer::verifiers::payment_route_sum::PaymentRouteSumVerifier.verify(&ctx)
    }

    assert!(matches!(
        check(&[(Some(0), 40), (Some(0), 60), (None, 75), (None, 25)]),
        VerifyResult::Pass
    ));
    let failure = check(&[(Some(0), 40), (Some(0), 60), (None, 90)]);
    assert!(matches!(failure, VerifyResult::Fail(message) if message.contains(TRACK_GUID)));
}
