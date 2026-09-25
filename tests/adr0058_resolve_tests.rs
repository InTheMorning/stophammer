// ADR 0058 task 004: the operator resolves a copy with `keep_source` or
// `relocate`. This mirrors the fixture style of
// tests/adr0058_copy_routes_tests.rs: a minimal verifier chain (content_hash
// only) so a synthetic feed ingests without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-adr0058-resolve-admin-token";

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0058-resolve-signer"));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN_TOKEN.into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// A synthetic feed with one track and one feed-level payment route.
/// `self_link_url` becomes the body's `self_feed` link when given.
/// `last_build_date` becomes the channel's `lastBuildDate` when given.
fn feed_data(
    feed_guid: &str,
    title: &str,
    track_guid: &str,
    route_address: &str,
    self_link_url: Option<&str>,
    last_build_date: Option<i64>,
) -> serde_json::Value {
    let links = self_link_url.map_or_else(
        || serde_json::json!([]),
        |url| {
            serde_json::json!([{
                "position": 0,
                "link_type": "self_feed",
                "url": url,
                "extraction_path": "feed.atom:link[@rel='self']"
            }])
        },
    );
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "last_build_date": last_build_date,
        "links": links,
        "feed_payment_routes": [{
            "recipient_name": null,
            "route_type": "keysend",
            "address": route_address,
            "custom_key": null,
            "custom_value": null,
            "split": 100,
            "fee": false
        }],
        "tracks": [{
            "track_guid": track_guid,
            "title": "Track One",
            "explicit": false
        }]
    })
}

fn ingest_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": feed_data,
    })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    if bytes.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    admin_token: Option<&str>,
    body: Option<&serde_json::Value>,
) -> (http::StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = admin_token {
        builder = builder.header("X-Admin-Token", token);
    }
    let request = if let Some(body) = body {
        builder = builder.header("Content-Type", "application/json");
        builder
            .body(Body::from(serde_json::to_vec(body).expect("serialize")))
            .expect("build request")
    } else {
        builder.body(Body::empty()).expect("build request")
    };
    let resp = app.oneshot(request).await.expect("send request");
    let status = resp.status();
    let body = body_json(resp).await;
    (status, body)
}

/// Ingests `data` at `url`, used as both `canonical_url` and `source_url`,
/// and asserts it is accepted.
async fn accept_source(
    app: axum::Router,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    data: &serde_json::Value,
) {
    let payload = ingest_payload(url, url, crawl_token, content_hash, data);
    let (status, body) = send(app, "POST", "/ingest/feed", None, Some(&payload)).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

/// Submits `data` at `url`, used as both `canonical_url` and `source_url`.
/// Returns the raw ingest response for the caller to inspect.
async fn submit(
    app: axum::Router,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    data: &serde_json::Value,
) -> serde_json::Value {
    let payload = ingest_payload(url, url, crawl_token, content_hash, data);
    let (status, body) = send(app, "POST", "/ingest/feed", None, Some(&payload)).await;
    assert_eq!(status, http::StatusCode::OK);
    body
}

fn stored_feed(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> stophammer::model::Feed {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed_by_guid(&conn, feed_guid)
        .expect("query feed")
        .expect("feed row must exist")
}

fn declared_self_url(db: &Arc<Mutex<rusqlite::Connection>>, feed_guid: &str) -> Option<String> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_declared_self_url(&conn, feed_guid).expect("query declared_self_url")
}

fn feed_copy_row(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
    url: &str,
) -> Option<stophammer::db::FeedCopyRow> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_feed_copy(&conn, feed_guid, url).expect("query feed_copies")
}

fn feed_upserted_events(
    db: &Arc<Mutex<rusqlite::Connection>>,
    feed_guid: &str,
) -> Vec<stophammer::event::Event> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_events_since(&conn, 0, 10_000)
        .expect("read events")
        .into_iter()
        .filter(|ev| {
            ev.event_type == stophammer::event::EventType::FeedUpserted
                && ev.subject_guid == feed_guid
        })
        .collect()
}

fn upserted_feed_reason(event: &stophammer::event::Event) -> Option<String> {
    match &event.payload {
        stophammer::event::EventPayload::FeedUpserted(p) => p.reason.clone(),
        other => panic!("expected a FeedUpserted payload, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. A resolve with no admin token answers 403, and changes nothing.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_with_no_admin_token_is_forbidden_and_changes_nothing() {
    let crawl_token = "adr0058-resolve-forbidden-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-forbidden-guid";
    let url_a = "https://example.com/adr0058-resolve-forbidden/a.xml";
    let url_b = "https://example.com/adr0058-resolve-forbidden/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Victim Feed",
            "track-a",
            "victim@ln.example",
            None,
            None,
        ),
    )
    .await;
    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid,
            "Attacker Feed",
            "track-a",
            "attacker@ln.example",
            None,
            None,
        ),
    )
    .await;

    let (status, _) = send(
        app,
        "POST",
        &format!("/v1/feeds/{feed_guid}/copies/resolve"),
        None,
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "keep_source",
            "reason": "should not apply"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::FORBIDDEN);

    assert_eq!(stored_feed(&db, feed_guid).feed_url, url_a);
    let row = feed_copy_row(&db, feed_guid, url_b).expect("row must exist");
    assert_eq!(row.resolution, None, "no admin token must resolve nothing");
}

// ---------------------------------------------------------------------------
// 2. keep_source resolves with the row's current digest, and the record does
//    not change.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn keep_source_resolves_with_the_current_digest_and_the_feed_does_not_change() {
    let crawl_token = "adr0058-resolve-keep-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-keep-guid";
    let url_a = "https://example.com/adr0058-resolve-keep/a.xml";
    let url_b = "https://example.com/adr0058-resolve-keep/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Victim Feed",
            "track-a",
            "victim@ln.example",
            None,
            None,
        ),
    )
    .await;
    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid,
            "Attacker Feed",
            "track-a",
            "attacker@ln.example",
            None,
            None,
        ),
    )
    .await;

    let digest_before = feed_copy_row(&db, feed_guid, url_b)
        .expect("row must exist")
        .summary_digest;

    let (status, body) = send(
        app,
        "POST",
        &format!("/v1/feeds/{feed_guid}/copies/resolve"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "keep_source",
            "reason": "confirmed this is an impersonation; keeping the held record"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK, "{body:?}");
    let event_ids = body["event_ids"].as_array().expect("event_ids array");
    assert_eq!(event_ids.len(), 1, "keep_source signs one event: {body:?}");

    let row = feed_copy_row(&db, feed_guid, url_b).expect("row must exist");
    assert_eq!(row.resolution.as_deref(), Some("keep_source"));
    assert_eq!(row.resolved_digest.as_deref(), Some(digest_before.as_str()));

    assert_eq!(
        stored_feed(&db, feed_guid).feed_url,
        url_a,
        "keep_source must not change the record"
    );
}

// ---------------------------------------------------------------------------
// 3. relocate sets feed_url to the row's URL, clears last_build_date and
//    declared_self_url, and the FeedUpserted event carries the reason.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn relocate_moves_the_feed_and_the_event_carries_the_reason() {
    let crawl_token = "adr0058-resolve-relocate-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-relocate-guid";
    let url_a = "https://example.com/adr0058-resolve-relocate/a.xml";
    let url_b = "https://example.com/adr0058-resolve-relocate/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Victim Feed",
            "track-a",
            "victim@ln.example",
            None,
            Some(5000),
        ),
    )
    .await;
    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid,
            "New Feed",
            "track-a",
            "new@ln.example",
            None,
            None,
        ),
    )
    .await;

    let reason = "confirmed move to the new host";
    let (status, body) = send(
        app,
        "POST",
        &format!("/v1/feeds/{feed_guid}/copies/resolve"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "relocate",
            "reason": reason
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK, "{body:?}");
    let event_ids = body["event_ids"].as_array().expect("event_ids array");
    assert_eq!(event_ids.len(), 2, "relocate signs two events: {body:?}");

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(feed.feed_url, url_b);
    assert_eq!(feed.last_build_date, None);
    assert_eq!(declared_self_url(&db, feed_guid), None);

    let upserted = feed_upserted_events(&db, feed_guid);
    let latest = upserted.last().expect("a FeedUpserted event must exist");
    assert_eq!(upserted_feed_reason(latest).as_deref(), Some(reason));
}

// ---------------------------------------------------------------------------
// 4. After relocate, a body from the new URL with an older last_build_date
//    than the value before the relocation is accepted as an update.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn after_relocate_an_older_last_build_date_from_the_new_url_is_accepted() {
    let crawl_token = "adr0058-resolve-stale-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-stale-guid";
    let url_a = "https://example.com/adr0058-resolve-stale/a.xml";
    let url_b = "https://example.com/adr0058-resolve-stale/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Victim Feed",
            "track-a",
            "victim@ln.example",
            None,
            Some(5000),
        ),
    )
    .await;
    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid,
            "New Feed",
            "track-a",
            "new@ln.example",
            None,
            None,
        ),
    )
    .await;
    let (status, body) = send(
        app.clone(),
        "POST",
        &format!("/v1/feeds/{feed_guid}/copies/resolve"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "relocate",
            "reason": "confirmed move to the new host"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK, "{body:?}");
    assert_eq!(stored_feed(&db, feed_guid).last_build_date, None);

    // An older last_build_date than the pre-relocation value of 5000 must
    // still be accepted: the stale rule of ADR 0053 section 3 only fires
    // when the stored record already carries a last_build_date.
    let update_body = send(
        app,
        "POST",
        "/ingest/feed",
        None,
        Some(&ingest_payload(
            url_b,
            url_b,
            crawl_token,
            "hash-c",
            &feed_data(
                feed_guid,
                "New Feed",
                "track-a",
                "new@ln.example",
                None,
                Some(1000),
            ),
        )),
    )
    .await
    .1;
    assert_eq!(
        update_body["accepted"].as_bool(),
        Some(true),
        "an older last_build_date must be accepted after relocation clears it: {update_body:?}"
    );
    assert_eq!(stored_feed(&db, feed_guid).last_build_date, Some(1000));
}

// ---------------------------------------------------------------------------
// 5. After relocate, a body from the old URL is a mirror, and a record whose
//    old source declared the old URL as its self link does not move back.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn after_relocate_the_old_url_is_a_mirror_and_does_not_move_the_record_back() {
    let crawl_token = "adr0058-resolve-noback-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-noback-guid";
    let url_a = "https://example.com/adr0058-resolve-noback/a.xml";
    let url_b = "https://example.com/adr0058-resolve-noback/b.xml";

    // The source at url_a declares itself as its own self link, so
    // declared_self_url == url_a before the relocation.
    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Victim Feed",
            "track-a",
            "victim@ln.example",
            Some(url_a),
            None,
        ),
    )
    .await;
    assert_eq!(declared_self_url(&db, feed_guid).as_deref(), Some(url_a));

    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid,
            "New Feed",
            "track-a",
            "new@ln.example",
            None,
            None,
        ),
    )
    .await;
    let (status, _) = send(
        app.clone(),
        "POST",
        &format!("/v1/feeds/{feed_guid}/copies/resolve"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "relocate",
            "reason": "confirmed move to the new host"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(declared_self_url(&db, feed_guid), None);

    // A body from the old URL is now a mirror. Because declared_self_url was
    // cleared, ADR 0052 section 2 finds no self-link match, and the record
    // does not move back.
    let mirror_body = send(
        app,
        "POST",
        "/ingest/feed",
        None,
        Some(&ingest_payload(
            url_a,
            url_a,
            crawl_token,
            "hash-c",
            &feed_data(
                feed_guid,
                "Victim Feed",
                "track-a",
                "victim@ln.example",
                Some(url_a),
                None,
            ),
        )),
    )
    .await
    .1;
    assert_eq!(
        mirror_body["accepted"].as_bool(),
        Some(false),
        "{mirror_body:?}"
    );

    assert_eq!(
        stored_feed(&db, feed_guid).feed_url,
        url_b,
        "the old URL's self link must not move the record back after relocation"
    );
}

// ---------------------------------------------------------------------------
// 6. relocate to a URL that another record holds as its source answers 409,
//    and changes nothing.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn relocate_to_a_url_another_record_holds_answers_409_and_changes_nothing() {
    let crawl_token = "adr0058-resolve-conflict-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid_1 = "adr0058-resolve-conflict-guid-1";
    let feed_guid_2 = "adr0058-resolve-conflict-guid-2";
    let url_a = "https://example.com/adr0058-resolve-conflict/a.xml";
    let url_b = "https://example.com/adr0058-resolve-conflict/b.xml";

    // feed_guid_1's own record, at url_a.
    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid_1,
            "Feed One",
            "track-a",
            "one@ln.example",
            None,
            None,
        ),
    )
    .await;
    // A mirror of feed_guid_1 at url_b, while no other record holds url_b.
    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid_1,
            "Mirror Feed",
            "track-a",
            "mirror@ln.example",
            None,
            None,
        ),
    )
    .await;
    assert!(
        feed_copy_row(&db, feed_guid_1, url_b).is_some(),
        "the mirror must have written a feed_copies row"
    );
    // feed_guid_2's own record is created afterwards, at url_b.
    accept_source(
        app.clone(),
        url_b,
        crawl_token,
        "hash-c",
        &feed_data(
            feed_guid_2,
            "Feed Two",
            "track-b",
            "two@ln.example",
            None,
            None,
        ),
    )
    .await;

    let (status, _) = send(
        app,
        "POST",
        &format!("/v1/feeds/{feed_guid_1}/copies/resolve"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "relocate",
            "reason": "should not apply"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::CONFLICT);

    assert_eq!(stored_feed(&db, feed_guid_1).feed_url, url_a);
    assert_eq!(stored_feed(&db, feed_guid_2).feed_url, url_b);
    let row = feed_copy_row(&db, feed_guid_1, url_b).expect("row must exist");
    assert_eq!(row.resolution, None, "a 409 must resolve nothing");
}

// ---------------------------------------------------------------------------
// 7. PATCH with feed_url and no reason answers 400. With a reason, it clears
//    the two fields.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn patch_feed_url_with_no_reason_answers_400() {
    let crawl_token = "adr0058-resolve-patch-400-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-patch-400-guid";
    let url_a = "https://example.com/adr0058-resolve-patch-400/a.xml";
    let url_b = "https://example.com/adr0058-resolve-patch-400/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(feed_guid, "Feed", "track-a", "a@ln.example", None, None),
    )
    .await;

    let (status, _) = send(
        app,
        "PATCH",
        &format!("/v1/feeds/{feed_guid}"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({ "feed_url": url_b })),
    )
    .await;
    assert_eq!(status, http::StatusCode::BAD_REQUEST);
    assert_eq!(stored_feed(&db, feed_guid).feed_url, url_a);
}

#[tokio::test]
async fn patch_feed_url_with_a_reason_clears_last_build_date_and_declared_self_url() {
    let crawl_token = "adr0058-resolve-patch-ok-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-patch-ok-guid";
    let url_a = "https://example.com/adr0058-resolve-patch-ok/a.xml";
    let url_b = "https://example.com/adr0058-resolve-patch-ok/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Feed",
            "track-a",
            "a@ln.example",
            Some(url_a),
            Some(5000),
        ),
    )
    .await;
    assert_eq!(declared_self_url(&db, feed_guid).as_deref(), Some(url_a));

    let (status, _) = send(
        app,
        "PATCH",
        &format!("/v1/feeds/{feed_guid}"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "feed_url": url_b,
            "reason": "confirmed move to the new host"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::NO_CONTENT);

    let feed = stored_feed(&db, feed_guid);
    assert_eq!(feed.feed_url, url_b);
    assert_eq!(feed.last_build_date, None);
    assert_eq!(declared_self_url(&db, feed_guid), None);
}

// ---------------------------------------------------------------------------
// 8. A replica that applies the events has the new feed_url, a null
//    last_build_date, and the resolution.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_replica_applying_the_events_has_the_relocation_and_the_resolution() {
    let crawl_token = "adr0058-resolve-replica-token";
    let db1 = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db1), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-resolve-replica-guid";
    let url_a = "https://example.com/adr0058-resolve-replica/a.xml";
    let url_b = "https://example.com/adr0058-resolve-replica/b.xml";

    accept_source(
        app.clone(),
        url_a,
        crawl_token,
        "hash-a",
        &feed_data(
            feed_guid,
            "Feed",
            "track-a",
            "a@ln.example",
            None,
            Some(5000),
        ),
    )
    .await;
    submit(
        app.clone(),
        url_b,
        crawl_token,
        "hash-b",
        &feed_data(
            feed_guid,
            "New Feed",
            "track-a",
            "new@ln.example",
            None,
            None,
        ),
    )
    .await;
    let (status, _) = send(
        app,
        "POST",
        &format!("/v1/feeds/{feed_guid}/copies/resolve"),
        Some(ADMIN_TOKEN),
        Some(&serde_json::json!({
            "url": url_b,
            "decision": "relocate",
            "reason": "confirmed move to the new host"
        })),
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);

    let events: Vec<stophammer::event::Event> = {
        let conn = db1.lock().expect("lock db1");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events")
    };
    assert!(
        events
            .iter()
            .any(|ev| ev.event_type == stophammer::event::EventType::FeedCopyResolved),
        "the primary must have signed a FeedCopyResolved event"
    );

    let db2 = common::test_db_arc();
    let pool2 = common::wrap_pool(Arc::clone(&db2));
    for ev in &events {
        let result = stophammer::apply::apply_single_event(&pool2, ev);
        assert!(
            result.is_ok(),
            "apply on the replica should succeed: {result:?}"
        );
    }

    let replica_feed = stored_feed(&db2, feed_guid);
    assert_eq!(replica_feed.feed_url, url_b);
    assert_eq!(replica_feed.last_build_date, None);

    let replica_row = feed_copy_row(&db2, feed_guid, url_b).expect("row must replicate");
    assert_eq!(replica_row.resolution.as_deref(), Some("relocate"));
}
