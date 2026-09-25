// ADR 0058 task 003: the public copy routes.
//
// `GET /v1/feeds/{guid}/copies`, `GET /v1/copies`, and `copy_count` in the
// feed detail response. This mirrors the fixture style of
// tests/adr0058_ingest_copy_tests.rs: a minimal verifier chain (content_hash
// only) so a synthetic feed ingests without needing the full default chain.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0058-copy-routes-signer"));
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
        admin_token: "test-adr0058-copy-routes-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// A synthetic feed with one track and one feed-level payment route. The
/// route carries `custom_key`/`custom_value` so a test can change one
/// recipient's identity without changing the track set.
fn feed_data_with_recipient(
    feed_guid: &str,
    title: &str,
    track_guid: &str,
    route_address: &str,
    custom_key: Option<&str>,
    custom_value: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": feed_guid,
        "title": title,
        "raw_medium": "episodic",
        "explicit": false,
        "feed_payment_routes": [{
            "recipient_name": null,
            "route_type": "keysend",
            "address": route_address,
            "custom_key": custom_key,
            "custom_value": custom_value,
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
    body: Option<&serde_json::Value>,
) -> (http::StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
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

/// Ingests `feed_data` at `url`, used as both `canonical_url` and
/// `source_url`, and asserts it is accepted. Seeds the source record of a
/// GUID.
async fn accept_source(
    app: axum::Router,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) {
    let payload = ingest_payload(url, url, crawl_token, content_hash, feed_data);
    let (status, body) = send(app, "POST", "/ingest/feed", Some(&payload)).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

/// Submits `feed_data` at `url`, used as both `canonical_url` and
/// `source_url`. Used for a mirror submission, which the node never applies
/// as an update.
async fn submit_mirror(
    app: axum::Router,
    url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    let payload = ingest_payload(url, url, crawl_token, content_hash, feed_data);
    let (status, body) = send(app, "POST", "/ingest/feed", Some(&payload)).await;
    assert_eq!(status, http::StatusCode::OK);
    body
}

// ---------------------------------------------------------------------------
// 1. A copy with a different recipient is open, counted, and listed.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_copy_with_a_different_recipient_is_open_counted_and_listed() {
    let crawl_token = "adr0058-routes-diff-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-routes-diff-guid";
    let url_a = "https://example.com/adr0058-routes-diff/a.xml";
    let url_b = "https://example.com/adr0058-routes-diff/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
    );
    accept_source(app.clone(), url_a, crawl_token, "hash-a", &feed_data_a).await;

    let feed_data_b = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
    );
    submit_mirror(app.clone(), url_b, crawl_token, "hash-b", &feed_data_b).await;

    let (status, body) = send(
        app.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}/copies"),
        None,
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    let rows = body["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 1, "one row for url_b: {rows:?}");
    assert_eq!(rows[0]["url"], url_b);
    assert_eq!(rows[0]["differs_tracks"], false);
    assert_eq!(rows[0]["differs_recipients"], true);
    assert_eq!(rows[0]["open"], true);
    assert_eq!(rows[0]["guid_origin"], false);
    assert_eq!(rows[0]["resolution"], serde_json::Value::Null);
    assert_eq!(body["copies_over_limit"], 0);

    let (status, feed_body) =
        send(app.clone(), "GET", &format!("/v1/feeds/{feed_guid}"), None).await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(feed_body["data"]["copy_count"], 1, "{feed_body:?}");

    let (status, list_body) = send(app, "GET", "/v1/copies", None).await;
    assert_eq!(status, http::StatusCode::OK);
    let items = list_body["data"].as_array().expect("data array");
    let item = items
        .iter()
        .find(|item| item["feed_guid"] == feed_guid)
        .unwrap_or_else(|| panic!("record must be listed in /v1/copies: {items:?}"));
    assert_eq!(item["copy_count"], 1);
    assert_eq!(item["copies_over_limit"], 0);
    assert_eq!(item["feed_url"], url_a);
}

// ---------------------------------------------------------------------------
// 2. An alias with the same item GUIDs and recipients is not open and not
//    listed.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_alias_is_not_open_and_not_listed() {
    let crawl_token = "adr0058-routes-alias-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-routes-alias-guid";
    let url_a = "https://example.com/adr0058-routes-alias/a.xml";
    let url_b = "https://example.com/adr0058-routes-alias/b.xml";

    let feed_data = feed_data_with_recipient(
        feed_guid,
        "Same Feed",
        "track-a",
        "same@ln.example",
        None,
        None,
    );
    accept_source(app.clone(), url_a, crawl_token, "hash-a", &feed_data).await;
    submit_mirror(app.clone(), url_b, crawl_token, "hash-b", &feed_data).await;

    let (status, body) = send(
        app.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}/copies"),
        None,
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    let rows = body["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["differs_tracks"], false);
    assert_eq!(rows[0]["differs_recipients"], false);
    assert_eq!(rows[0]["open"], false);

    let (_, feed_body) = send(app.clone(), "GET", &format!("/v1/feeds/{feed_guid}"), None).await;
    assert_eq!(feed_body["data"]["copy_count"], 0, "{feed_body:?}");

    let (_, list_body) = send(app, "GET", "/v1/copies", None).await;
    let items = list_body["data"].as_array().expect("data array");
    assert!(
        !items.iter().any(|item| item["feed_guid"] == feed_guid),
        "an alias-only record must not be listed in /v1/copies: {items:?}"
    );
}

// ---------------------------------------------------------------------------
// 3. A copy that differs only in a keysend custom_value is open.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_copy_that_differs_only_in_custom_value_is_open() {
    let crawl_token = "adr0058-routes-custom-value-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-routes-custom-value-guid";
    let url_a = "https://example.com/adr0058-routes-custom-value/a.xml";
    let url_b = "https://example.com/adr0058-routes-custom-value/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "03aaaa",
        Some("696969"),
        Some("aaa111"),
    );
    accept_source(app.clone(), url_a, crawl_token, "hash-a", &feed_data_a).await;

    let feed_data_b = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "03aaaa",
        Some("696969"),
        Some("bbb222"),
    );
    submit_mirror(app.clone(), url_b, crawl_token, "hash-b", &feed_data_b).await;

    let (status, body) = send(app, "GET", &format!("/v1/feeds/{feed_guid}/copies"), None).await;
    assert_eq!(status, http::StatusCode::OK);
    let rows = body["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["differs_tracks"], false,
        "the track set is unchanged: {rows:?}"
    );
    assert_eq!(
        rows[0]["differs_recipients"], true,
        "a changed custom_value alone must differ: {rows:?}"
    );
    assert_eq!(rows[0]["open"], true);
}

// ---------------------------------------------------------------------------
// 4. `guid_origin` is true for the row whose URL is the UUIDv5 origin of the
//    GUID.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn guid_origin_is_true_for_the_url_that_derives_the_guid() {
    let crawl_token = "adr0058-routes-origin-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "7192ec54-3aa2-5c61-987b-51bf75f68568";
    let source_url =
        "https://musicsideproject.com/api/hosted/7192ec54-3aa2-5c61-987b-51bf75f68568.xml";
    let origin_url = "https://wavlake.com/feed/music/a82acc2f-3440-491c-94c2-d27bebf6cfe6";

    let feed_data_source = feed_data_with_recipient(
        feed_guid,
        "Source Feed",
        "track-a",
        "source@ln.example",
        None,
        None,
    );
    accept_source(
        app.clone(),
        source_url,
        crawl_token,
        "hash-a",
        &feed_data_source,
    )
    .await;

    let feed_data_origin = feed_data_with_recipient(
        feed_guid,
        "Origin Feed",
        "track-a",
        "origin@ln.example",
        None,
        None,
    );
    submit_mirror(
        app.clone(),
        origin_url,
        crawl_token,
        "hash-b",
        &feed_data_origin,
    )
    .await;

    let (status, body) = send(app, "GET", &format!("/v1/feeds/{feed_guid}/copies"), None).await;
    assert_eq!(status, http::StatusCode::OK);
    let rows = body["data"].as_array().expect("data array");
    let row = rows
        .iter()
        .find(|row| row["url"] == origin_url)
        .unwrap_or_else(|| panic!("row for origin_url must exist: {rows:?}"));
    assert_eq!(row["guid_origin"], true, "{row:?}");
}

// ---------------------------------------------------------------------------
// 5. A resolution closes the copy, and a changed summary opens it again.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_resolution_closes_the_copy_and_a_changed_summary_opens_it_again() {
    let crawl_token = "adr0058-routes-resolve-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-routes-resolve-guid";
    let url_a = "https://example.com/adr0058-routes-resolve/a.xml";
    let url_b = "https://example.com/adr0058-routes-resolve/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
    );
    accept_source(app.clone(), url_a, crawl_token, "hash-a", &feed_data_a).await;

    let feed_data_b1 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
    );
    submit_mirror(app.clone(), url_b, crawl_token, "hash-b1", &feed_data_b1).await;

    // Resolve the copy directly against the store, standing in for the
    // POST /v1/feeds/{guid}/copies/resolve route of task 004, which is not
    // built yet. This is the same DB effect a FeedCopyResolved event
    // applies (src/apply.rs).
    let digest_before = {
        let conn = db.lock().expect("lock db");
        stophammer::db::get_feed_copy(&conn, feed_guid, url_b)
            .expect("query feed_copies")
            .expect("row must exist")
            .summary_digest
    };
    {
        let conn = db.lock().expect("lock db");
        stophammer::db::set_feed_copy_resolution(
            &conn,
            feed_guid,
            url_b,
            "keep_source",
            "operator checked it and kept the source",
            stophammer::db::unix_now(),
            &digest_before,
        )
        .expect("set resolution");
    }

    let (status, body) = send(
        app.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}/copies"),
        None,
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    let rows = body["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["open"], false,
        "a held resolution closes the copy: {rows:?}"
    );
    assert_eq!(rows[0]["resolution"]["decision"], "keep_source");
    assert_eq!(
        rows[0]["resolution"]["reason"],
        "operator checked it and kept the source"
    );
    assert_eq!(rows[0]["resolution"]["current"], true);

    let (_, list_body) = send(app.clone(), "GET", "/v1/copies", None).await;
    let items = list_body["data"].as_array().expect("data array");
    assert!(
        !items.iter().any(|item| item["feed_guid"] == feed_guid),
        "a closed copy with no overflow must not be listed: {items:?}"
    );

    // A new summary at url_b changes the digest, so the held resolution no
    // longer matches, and the copy is open again.
    let feed_data_b2 = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker-2@ln.example",
        None,
        None,
    );
    submit_mirror(app.clone(), url_b, crawl_token, "hash-b2", &feed_data_b2).await;

    let (status, body2) = send(app, "GET", &format!("/v1/feeds/{feed_guid}/copies"), None).await;
    assert_eq!(status, http::StatusCode::OK);
    let rows2 = body2["data"].as_array().expect("data array");
    assert_eq!(rows2.len(), 1);
    assert_eq!(
        rows2[0]["open"], true,
        "a changed summary must open the copy again: {rows2:?}"
    );
    assert_eq!(
        rows2[0]["resolution"]["current"], false,
        "the held resolution no longer matches the new digest: {rows2:?}"
    );
    assert_eq!(rows2[0]["resolution"]["decision"], "keep_source");
}

// ---------------------------------------------------------------------------
// 6. A record whose counter is above zero is listed, even with no open copy.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_record_with_only_an_overflow_counter_is_listed() {
    let crawl_token = "adr0058-routes-overflow-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);
    let feed_guid = "adr0058-routes-overflow-guid";
    let url_a = "https://example.com/adr0058-routes-overflow/source.xml";

    // Every mirror below carries the exact same tracks and recipients as the
    // source, so every row this test makes is an alias: copy_count stays 0
    // even after the counter goes above zero.
    let feed_data = feed_data_with_recipient(
        feed_guid,
        "Same Feed",
        "track-a",
        "same@ln.example",
        None,
        None,
    );
    accept_source(app.clone(), url_a, crawl_token, "hash-a", &feed_data).await;

    let max_rows = usize::try_from(stophammer::db::MAX_COPIES_PER_GUID)
        .expect("MAX_COPIES_PER_GUID fits usize");
    for i in 0..max_rows {
        let url = format!("https://example.com/adr0058-routes-overflow/mirror-{i}.xml");
        submit_mirror(
            app.clone(),
            &url,
            crawl_token,
            &format!("hash-{i}"),
            &feed_data,
        )
        .await;
    }
    let overflow_url = "https://example.com/adr0058-routes-overflow/mirror-overflow.xml";
    submit_mirror(
        app.clone(),
        overflow_url,
        crawl_token,
        "hash-overflow",
        &feed_data,
    )
    .await;

    let (status, feed_copies_body) = send(
        app.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}/copies"),
        None,
    )
    .await;
    assert_eq!(status, http::StatusCode::OK);
    assert_eq!(feed_copies_body["copies_over_limit"], 1);
    for row in feed_copies_body["data"].as_array().expect("data array") {
        assert_eq!(
            row["open"], false,
            "identical content must be an alias: {row:?}"
        );
    }

    let (_, feed_body) = send(app.clone(), "GET", &format!("/v1/feeds/{feed_guid}"), None).await;
    assert_eq!(feed_body["data"]["copy_count"], 0, "{feed_body:?}");

    let (status, list_body) = send(app, "GET", "/v1/copies", None).await;
    assert_eq!(status, http::StatusCode::OK);
    let items = list_body["data"].as_array().expect("data array");
    let item = items
        .iter()
        .find(|item| item["feed_guid"] == feed_guid)
        .unwrap_or_else(|| panic!("an overflow-only record must be listed: {items:?}"));
    assert_eq!(item["copy_count"], 0);
    assert_eq!(item["copies_over_limit"], 1);
}

// ---------------------------------------------------------------------------
// 7. `/v1/feeds/{unknown}/copies` answers 404.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_feed_copies_route_answers_404() {
    let crawl_token = "adr0058-routes-404-token";
    let db = common::test_db_arc();
    let state = test_app_state(db, crawl_token);
    let app = stophammer::api::build_router(state);

    let (status, _body) = send(app, "GET", "/v1/feeds/does-not-exist/copies", None).await;
    assert_eq!(status, http::StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 8. A replica applying the same events gives the same answer from each
//    route, except last_seen and copies_over_limit.
// ---------------------------------------------------------------------------

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end replica comparison across two routes and two databases"
)]
async fn a_replica_gives_the_same_copy_routes_answer_except_last_seen_and_overflow() {
    let crawl_token = "adr0058-routes-replica-token";
    let db_a = common::test_db_arc();
    let state_a = test_app_state(Arc::clone(&db_a), crawl_token);
    let app_a = stophammer::api::build_router(Arc::clone(&state_a));
    let feed_guid = "adr0058-routes-replica-guid";
    let url_a = "https://example.com/adr0058-routes-replica/a.xml";
    let url_b = "https://example.com/adr0058-routes-replica/b.xml";

    let feed_data_a = feed_data_with_recipient(
        feed_guid,
        "Victim Feed",
        "track-a",
        "victim@ln.example",
        None,
        None,
    );
    accept_source(app_a.clone(), url_a, crawl_token, "hash-a", &feed_data_a).await;

    let feed_data_b = feed_data_with_recipient(
        feed_guid,
        "Attacker Feed",
        "track-a",
        "attacker@ln.example",
        None,
        None,
    );
    submit_mirror(app_a.clone(), url_b, crawl_token, "hash-b", &feed_data_b).await;

    let events: Vec<stophammer::event::Event> = {
        let conn = db_a.lock().expect("lock db_a");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("read events from A")
    };
    assert!(
        events.iter().any(|ev| matches!(
            ev.event_type,
            stophammer::event::EventType::FeedCopyObserved
        )),
        "the sequence must emit at least one FeedCopyObserved event, or this test proves nothing"
    );

    let db_b = common::test_db_arc();
    let pool_b = common::wrap_pool(Arc::clone(&db_b));
    for ev in &events {
        let result = stophammer::apply::apply_single_event(&pool_b, ev);
        assert!(
            result.is_ok(),
            "applying event {:?} on the replica must succeed: {result:?}",
            ev.event_type
        );
    }

    let state_b = test_app_state(Arc::clone(&db_b), "unused-b-token");
    let app_b = stophammer::api::build_router(state_b);

    let (status_a, body_a) = send(
        app_a.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}/copies"),
        None,
    )
    .await;
    let (status_b, body_b) = send(
        app_b.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}/copies"),
        None,
    )
    .await;
    assert_eq!(status_a, http::StatusCode::OK);
    assert_eq!(status_b, http::StatusCode::OK);

    let rows_a = body_a["data"].as_array().expect("data array A");
    let rows_b = body_b["data"].as_array().expect("data array B");
    assert_eq!(rows_a.len(), rows_b.len());
    for (row_a, row_b) in rows_a.iter().zip(rows_b.iter()) {
        let mut a = row_a.clone();
        let mut b = row_b.clone();
        a["last_seen"] = serde_json::Value::Null;
        b["last_seen"] = serde_json::Value::Null;
        assert_eq!(
            a, b,
            "a copy row must replicate identically except last_seen: a={row_a:?} b={row_b:?}"
        );
    }
    assert!(
        rows_a[0]["last_seen"].is_i64() || rows_a[0]["last_seen"].is_u64(),
        "the primary must set last_seen: {rows_a:?}"
    );
    assert_eq!(
        rows_b[0]["last_seen"],
        serde_json::Value::Null,
        "an applied FeedCopyObserved must never write last_seen on a replica: {rows_b:?}"
    );

    let (_, feed_a) = send(
        app_a.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}"),
        None,
    )
    .await;
    let (_, feed_b) = send(
        app_b.clone(),
        "GET",
        &format!("/v1/feeds/{feed_guid}"),
        None,
    )
    .await;
    assert_eq!(feed_a["data"]["copy_count"], feed_b["data"]["copy_count"]);

    let (_, list_a) = send(app_a, "GET", "/v1/copies", None).await;
    let (_, list_b) = send(app_b, "GET", "/v1/copies", None).await;
    let item_a = list_a["data"]
        .as_array()
        .expect("data array A")
        .iter()
        .find(|item| item["feed_guid"] == feed_guid)
        .expect("record must be listed on A")
        .clone();
    let item_b = list_b["data"]
        .as_array()
        .expect("data array B")
        .iter()
        .find(|item| item["feed_guid"] == feed_guid)
        .expect("record must be listed on B")
        .clone();
    assert_eq!(item_a["copy_count"], item_b["copy_count"]);
    assert_eq!(item_a["newest_first_seen"], item_b["newest_first_seen"]);
}
