// ADR 0079 task 001: a retire with no block keeps a hold, and a held GUID
// returns only from its held URL.
//
// The three retire paths of ADR 0079 section 1 are a `podcast:block` ingest,
// two gone answers (ADR 0067) and `DELETE /v1/feeds/{guid}?block=false`.
// The fixture style follows tests/adr0057_podcast_block_tests.rs: a minimal
// verifier chain (`content_hash` only).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

type Db = Arc<Mutex<rusqlite::Connection>>;

const CRAWL_TOKEN: &str = "adr0079-crawl-token";
const ADMIN_TOKEN: &str = "adr0079-admin-token";
const GONE_HOST: &str = "adr0079-gone.example.com";
const ONE_DAY_SECS: i64 = 24 * 60 * 60;

/// The `UUIDv5` namespace of `guid_origin_matches` (ADR 0052 section 4).
const GUID_ORIGIN_NAMESPACE: uuid::Uuid = uuid::uuid!("ead4c236-bf58-58c6-a2c6-a6b28d128cb6");

/// The three retire paths of ADR 0079 section 1.
#[derive(Clone, Copy, Debug)]
enum Retire {
    PodcastBlock,
    SourceGone,
    AdminNoBlock,
}

const ALL_RETIRES: [Retire; 3] = [
    Retire::PodcastBlock,
    Retire::SourceGone,
    Retire::AdminNoBlock,
];

fn state(db: &Db) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0079-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, CRAWL_TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(Arc::clone(db)),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: ADMIN_TOKEN.to_string(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: vec![GONE_HOST.to_string()],
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

fn feed_data(feed_guid: &str, blocked: bool) -> serde_json::Value {
    let mut data = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "ADR 0079 Feed",
        "raw_medium": "episodic",
        "explicit": false,
        "tracks": [{
            "track_guid": format!("{feed_guid}-track-01"),
            "title": "Track One",
            "explicit": false
        }]
    });
    if blocked {
        data["blocks"] = serde_json::json!([{"id": null, "value": "yes"}]);
    }
    data
}

fn payload(
    canonical_url: &str,
    source_url: &str,
    hash: &str,
    data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": CRAWL_TOKEN,
        "http_status": 200,
        "content_hash": hash,
        "feed_data": data,
    })
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    admin: bool,
    body: Option<&serde_json::Value>,
) -> (u16, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if admin {
        builder = builder.header("X-Admin-Token", ADMIN_TOKEN);
    }
    let request = match body {
        Some(body) => builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(body).expect("serialize")))
            .expect("build request"),
        None => builder.body(Body::empty()).expect("build request"),
    };
    let resp = app.oneshot(request).await.expect("send request");
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

async fn ingest(app: axum::Router, body: &serde_json::Value) -> serde_json::Value {
    let (status, json) = send(app, "POST", "/ingest/feed", false, Some(body)).await;
    assert_eq!(status, 200, "ingest must answer 200: {json:?}");
    json
}

fn count(db: &Db, sql: &str, arg: &str) -> i64 {
    let conn = db.lock().expect("lock db");
    conn.query_row(sql, [arg], |row| row.get(0)).expect("count")
}

fn feeds_rows(db: &Db, guid: &str) -> i64 {
    count(db, "SELECT COUNT(*) FROM feeds WHERE feed_guid = ?1", guid)
}

fn track_rows(db: &Db, guid: &str) -> i64 {
    count(db, "SELECT COUNT(*) FROM tracks WHERE feed_guid = ?1", guid)
}

fn hold_rows(db: &Db, guid: &str) -> i64 {
    count(
        db,
        "SELECT COUNT(*) FROM retired_source_holds WHERE feed_guid = ?1",
        guid,
    )
}

fn all_hold_rows(db: &Db) -> i64 {
    count(
        db,
        "SELECT COUNT(*) FROM retired_source_holds WHERE ?1 = ''",
        "",
    )
}

fn event_total(db: &Db) -> i64 {
    count(db, "SELECT COUNT(*) FROM events WHERE ?1 = ''", "")
}

fn hold(db: &Db, guid: &str) -> (String, String) {
    let conn = db.lock().expect("lock db");
    conn.query_row(
        "SELECT source_url, reason FROM retired_source_holds WHERE feed_guid = ?1",
        [guid],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("read hold")
}

fn last_retire_reason(db: &Db) -> String {
    let conn = db.lock().expect("lock db");
    let payload: String = conn
        .query_row(
            "SELECT payload_json FROM events WHERE event_type = 'feed_retired' \
             ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("read feed_retired payload");
    let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
    value["reason"].as_str().unwrap_or_default().to_string()
}

/// The feed-scoped identities of the tracks of `guid`, sorted.
fn track_identities(db: &Db, guid: &str) -> Vec<String> {
    let conn = db.lock().expect("lock db");
    let mut stmt = conn
        .prepare("SELECT track_guid FROM tracks WHERE feed_guid = ?1 ORDER BY track_guid")
        .expect("prepare");
    stmt.query_map([guid], |row| {
        row.get::<_, String>(0)
            .map(|track| stophammer::db::canonical_track_entity_id(guid, &track))
    })
    .expect("query")
    .collect::<Result<Vec<_>, _>>()
    .expect("collect")
}

/// A fixture after a retire: the held URL, the GUID and the track identities
/// before the retire.
struct Retired {
    db: Db,
    app: axum::Router,
    guid: String,
    url: String,
    identities: Vec<String>,
}

/// Ingests a feed and retires it by `how`. The result holds the app, so a
/// test can submit again.
async fn retired(how: Retire, label: &str) -> Retired {
    let db = common::test_db_arc();
    let app = stophammer::api::build_router(state(&db));
    let guid = format!("adr0079-{label}");
    let url = format!("https://{GONE_HOST}/{label}.xml");

    let first = ingest(
        app.clone(),
        &payload(&url, &url, "hash-seed", &feed_data(&guid, false)),
    )
    .await;
    assert_eq!(first["accepted"], true, "seed must be admitted: {first:?}");
    let identities = track_identities(&db, &guid);
    assert_eq!(identities.len(), 1, "the seed must have one track");

    match how {
        Retire::PodcastBlock => {
            let body = ingest(
                app.clone(),
                &payload(&url, &url, "hash-block", &feed_data(&guid, true)),
            )
            .await;
            assert_eq!(body["reason"], "source_blocked", "{body:?}");
        }
        Retire::SourceGone => {
            let gone = serde_json::json!({
                "canonical_url": url,
                "source_url": url,
                "crawl_token": CRAWL_TOKEN,
                "http_status": 404,
                "content_hash": "",
                "feed_data": null,
            });
            let body = ingest(app.clone(), &gone).await;
            assert_eq!(body["reason"], "source_gone_observed", "{body:?}");
            {
                let conn = db.lock().expect("lock db");
                conn.execute(
                    "UPDATE source_gone_answers SET first_gone_at = ?1 WHERE feed_guid = ?2",
                    rusqlite::params![stophammer::db::unix_now() - ONE_DAY_SECS, guid],
                )
                .expect("backdate");
            }
            let body = ingest(app.clone(), &gone).await;
            assert_eq!(body["reason"], "source_gone", "{body:?}");
        }
        Retire::AdminNoBlock => {
            let (status, _) = send(
                app.clone(),
                "DELETE",
                &format!("/v1/feeds/{guid}?block=false"),
                true,
                None,
            )
            .await;
            assert_eq!(status, 204, "the admin retire must succeed");
        }
    }
    assert_eq!(
        feeds_rows(&db, &guid),
        0,
        "{how:?}: the record must be gone"
    );
    Retired {
        db,
        app,
        guid,
        url,
        identities,
    }
}

// Check 1: each retire path with no block writes one hold.
#[tokio::test]
async fn each_retire_without_a_block_writes_one_hold() {
    for how in ALL_RETIRES {
        let r = retired(how, &format!("hold-{how:?}").to_lowercase()).await;
        assert_eq!(
            hold_rows(&r.db, &r.guid),
            1,
            "ADR 0079 §1: {how:?} must write one hold"
        );
        assert_eq!(all_hold_rows(&r.db), 1, "{how:?}: no other hold");
        let (source_url, reason) = hold(&r.db, &r.guid);
        assert_eq!(source_url, r.url, "{how:?}: the hold keeps the source URL");
        assert_eq!(
            reason,
            last_retire_reason(&r.db),
            "{how:?}: the hold keeps the reason of the event"
        );
        assert!(!reason.is_empty(), "{how:?}: the event has a reason");
    }
}

// Check 2: a retire with a block writes no hold.
#[tokio::test]
async fn a_retire_with_a_block_writes_no_hold() {
    let db = common::test_db_arc();
    let app = stophammer::api::build_router(state(&db));
    let guid = "adr0079-with-block";
    let url = format!("https://{GONE_HOST}/with-block.xml");
    ingest(
        app.clone(),
        &payload(&url, &url, "hash-seed", &feed_data(guid, false)),
    )
    .await;
    let (status, _) = send(app, "DELETE", &format!("/v1/feeds/{guid}"), true, None).await;
    assert_eq!(status, 204);
    assert_eq!(feeds_rows(&db, guid), 0);
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM feed_blocks WHERE ?1 = ''", ""),
        2,
        "the retire must write its two blocks"
    );
    assert_eq!(
        all_hold_rows(&db),
        0,
        "ADR 0079 §1: a retire with a block writes no hold"
    );
}

// Check 2: a retire by a GUID change writes no hold.
#[tokio::test]
async fn a_guid_change_retire_writes_no_hold() {
    let db = common::test_db_arc();
    let app = stophammer::api::build_router(state(&db));
    let url = "https://adr0079-guid-change.example.com/feed.xml";
    let old_guid = "11111111-1111-1111-1111-111111111179";
    let without_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let new_guid =
        uuid::Uuid::new_v5(&GUID_ORIGIN_NAMESPACE, without_scheme.as_bytes()).to_string();

    ingest(
        app.clone(),
        &payload(url, url, "hash-old", &feed_data(old_guid, false)),
    )
    .await;
    let body = ingest(
        app,
        &payload(url, url, "hash-new", &feed_data(&new_guid, false)),
    )
    .await;
    assert_eq!(
        body["accepted"], true,
        "the UUIDv5 change must pass: {body:?}"
    );
    assert_eq!(
        feeds_rows(&db, old_guid),
        0,
        "the old record must be retired"
    );
    assert_eq!(feeds_rows(&db, &new_guid), 1, "the new record must exist");
    assert_eq!(
        all_hold_rows(&db),
        0,
        "ADR 0079 §1: a GUID change retire writes no hold"
    );
}

// Check 3: the same GUID from a different URL gets `retired_source_conflict`.
#[tokio::test]
async fn a_different_url_gets_retired_source_conflict_and_writes_nothing() {
    for how in ALL_RETIRES {
        let r = retired(how, &format!("conflict-{how:?}").to_lowercase()).await;
        let other = "https://attacker.example.net/copy.xml";
        let events_before = event_total(&r.db);

        let body = ingest(
            r.app.clone(),
            &payload(other, other, "hash-attack", &feed_data(&r.guid, false)),
        )
        .await;
        assert_eq!(body["accepted"], false, "{how:?}: {body:?}");
        assert_eq!(body["no_change"], false, "{how:?}: {body:?}");
        assert_eq!(
            body["reason"], "retired_source_conflict",
            "ADR 0079 §2: {how:?}: {body:?}"
        );
        assert_eq!(
            body["events_emitted"].as_array().map(Vec::len),
            Some(0),
            "{how:?}: {body:?}"
        );
        assert_eq!(feeds_rows(&r.db, &r.guid), 0, "{how:?}: no record");
        assert_eq!(track_rows(&r.db, &r.guid), 0, "{how:?}: no track");
        assert_eq!(
            event_total(&r.db),
            events_before,
            "{how:?}: the event log must have no new event"
        );
        assert_eq!(hold_rows(&r.db, &r.guid), 1, "{how:?}: the hold stays");
        assert_eq!(
            count(
                &r.db,
                "SELECT COUNT(*) FROM feeds WHERE feed_url = ?1",
                other
            ),
            0,
            "{how:?}: the other URL must not be a source URL"
        );
    }
}

// Check 4: the held URL admits the feed, keeps the track identities and
// deletes the hold.
#[tokio::test]
async fn the_held_url_admits_the_feed_and_deletes_the_hold() {
    for how in ALL_RETIRES {
        let r = retired(how, &format!("return-{how:?}").to_lowercase()).await;
        let body = ingest(
            r.app.clone(),
            &payload(&r.url, &r.url, "hash-return", &feed_data(&r.guid, false)),
        )
        .await;
        assert_eq!(
            body["accepted"], true,
            "ADR 0079 §2: {how:?}: the held URL must be admitted: {body:?}"
        );
        assert_eq!(feeds_rows(&r.db, &r.guid), 1, "{how:?}: the record returns");
        assert_eq!(hold_rows(&r.db, &r.guid), 0, "{how:?}: the hold is deleted");
        assert_eq!(
            track_identities(&r.db, &r.guid),
            r.identities,
            "{how:?}: the track keeps its feed-scoped identity"
        );
    }
}

// Check 4: a `canonical_url` that is the held URL admits the feed when the
// `source_url` is a different URL.
#[tokio::test]
async fn a_canonical_url_equal_to_the_held_url_admits_the_feed() {
    let r = retired(Retire::AdminNoBlock, "canonical").await;
    let body = ingest(
        r.app.clone(),
        &payload(
            &r.url,
            "https://cdn.example.net/redirected.xml",
            "hash-canonical",
            &feed_data(&r.guid, false),
        ),
    )
    .await;
    assert_eq!(
        body["accepted"], true,
        "ADR 0079 §2: the held canonical URL must be admitted: {body:?}"
    );
    assert_eq!(feeds_rows(&r.db, &r.guid), 1);
    assert_eq!(hold_rows(&r.db, &r.guid), 0, "the hold is deleted");
}

// Check 5: the held URL with the block tag gets `source_blocked`, and the
// hold stays.
#[tokio::test]
async fn the_held_url_with_the_block_tag_is_blocked_and_the_hold_stays() {
    for how in ALL_RETIRES {
        let r = retired(how, &format!("blocked-{how:?}").to_lowercase()).await;
        let body = ingest(
            r.app.clone(),
            &payload(&r.url, &r.url, "hash-blocked", &feed_data(&r.guid, true)),
        )
        .await;
        assert_eq!(body["accepted"], false, "{how:?}: {body:?}");
        assert_eq!(
            body["reason"], "source_blocked",
            "ADR 0079 §2: {how:?}: {body:?}"
        );
        assert_eq!(feeds_rows(&r.db, &r.guid), 0, "{how:?}: no record");
        assert_eq!(hold_rows(&r.db, &r.guid), 1, "{how:?}: the hold stays");
    }
}

// Check 8: a community node that applies a `FeedRetired` event writes no hold.
#[tokio::test]
async fn a_replica_applying_feed_retired_writes_no_hold() {
    let r = retired(Retire::AdminNoBlock, "replica").await;
    assert_eq!(hold_rows(&r.db, &r.guid), 1, "the primary holds the GUID");

    let events = {
        let conn = r.db.lock().expect("lock db");
        stophammer::db::get_events_since(&conn, 0, 10_000).expect("events")
    };
    let replica = common::test_db_arc();
    let pool = common::wrap_pool(Arc::clone(&replica));
    for event in &events {
        stophammer::apply::apply_single_event(&pool, event).expect("apply event");
    }
    assert!(
        events
            .iter()
            .any(|e| e.event_type == stophammer::event::EventType::FeedRetired),
        "the log must hold a FeedRetired event"
    );
    assert_eq!(feeds_rows(&replica, &r.guid), 0, "the replica retired it");
    assert_eq!(
        all_hold_rows(&replica),
        0,
        "ADR 0079 §1: the apply of FeedRetired writes no hold"
    );
}

// ADR 0079 §1 and ADR 0046: the open creates the table when the migration
// runner skips migration 0055.
#[test]
fn the_open_creates_the_hold_table_when_the_migration_is_skipped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("skipped.db");
    drop(stophammer::db::open_db(&path));
    {
        let conn = rusqlite::Connection::open(&path).expect("open raw connection");
        conn.execute_batch("DROP TABLE retired_source_holds;")
            .expect("drop the hold table");
    }
    let conn = stophammer::db::open_db(&path);
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'retired_source_holds'",
            [],
            |row| row.get(0),
        )
        .expect("read sqlite_master");
    assert!(
        exists,
        "ADR 0079 §1 and ADR 0046: open_db must create retired_source_holds when migration 0055 did not run"
    );
}

// ADR 0079 §3: the release route. Check 6 and the contract rows.
#[tokio::test]
async fn a_release_admits_a_different_url_as_a_new_feed() {
    let r = retired(Retire::AdminNoBlock, "release-new").await;
    let uri = format!("/v1/retired-source-holds/{}", r.guid);
    let (status, _) = send(r.app.clone(), "DELETE", &uri, true, None).await;
    assert_eq!(status, 204, "ADR 0079 §3: the release must give 204");

    let other = "https://other.example.net/release.xml";
    let body = ingest(
        r.app.clone(),
        &payload(other, other, "hash-other", &feed_data(&r.guid, false)),
    )
    .await;
    assert_eq!(
        body["accepted"], true,
        "ADR 0079 §3: after the release a different URL must be admitted: {body:?}"
    );
    assert_eq!(feeds_rows(&r.db, &r.guid), 1, "the feed is a new feed");
}

#[tokio::test]
async fn a_release_gives_204_deletes_the_row_and_signs_no_event() {
    let r = retired(Retire::SourceGone, "release-row").await;
    let upper = r.guid.to_uppercase();
    let uri = format!("/v1/retired-source-holds/{upper}");
    let events_before = event_total(&r.db);

    let (status, _) = send(r.app.clone(), "DELETE", &uri, true, None).await;
    assert_eq!(status, 204, "ADR 0079 §3: the release must give 204");
    assert_eq!(hold_rows(&r.db, &r.guid), 0, "the release deletes the row");
    assert_eq!(
        event_total(&r.db),
        events_before,
        "ADR 0079 §3: the release must sign no event"
    );

    let (status, _) = send(r.app.clone(), "DELETE", &uri, true, None).await;
    assert_eq!(status, 404, "a second release must give 404");
}

#[tokio::test]
async fn a_release_needs_the_admin_token() {
    let r = retired(Retire::AdminNoBlock, "release-auth").await;
    let uri = format!("/v1/retired-source-holds/{}", r.guid);

    let (status, _) = send(r.app.clone(), "DELETE", &uri, false, None).await;
    assert_eq!(status, 403, "no admin token must give 403");

    let request = Request::builder()
        .method("DELETE")
        .uri(&uri)
        .header("X-Admin-Token", "wrong-token")
        .body(Body::empty())
        .expect("build request");
    let resp = r.app.clone().oneshot(request).await.expect("send request");
    assert_eq!(resp.status().as_u16(), 403, "a wrong token must give 403");
    assert_eq!(hold_rows(&r.db, &r.guid), 1, "the row stays");
}

#[test]
fn the_contract_has_the_release_route_and_the_reason() {
    let spec = serde_json::to_value(stophammer::openapi::primary_document())
        .expect("serialize the document");
    assert!(
        spec["paths"]["/v1/retired-source-holds/{guid}"]["delete"].is_object(),
        "ADR 0079 §3: the OpenAPI document must give the delete method of the release route"
    );
    let ingest_description =
        spec["paths"]["/ingest/feed"]["post"]["responses"]["200"]["description"]
            .as_str()
            .expect("ingest description");
    assert!(
        ingest_description.contains("retired_source_conflict"),
        "ADR 0079 §2: the ingest description must list retired_source_conflict"
    );
}

// ADR 0079 §4: the fill of the holds of earlier retires. The fixtures write
// events straight into the log, as a database from before the hold has them.

fn log_upserted(db: &Db, guid: &str, url: &str, seq: i64) {
    let payload = serde_json::json!({"feed": {"feed_guid": guid, "feed_url": url}});
    log_event(db, "feed_upserted", guid, &payload.to_string(), seq);
}

fn log_retired(db: &Db, guid: &str, reason: &str, seq: i64) {
    let payload = serde_json::json!({"feed_guid": guid, "reason": reason});
    log_event(db, "feed_retired", guid, &payload.to_string(), seq);
}

fn log_event(db: &Db, event_type: &str, guid: &str, payload_json: &str, seq: i64) {
    let conn = db.lock().expect("lock db");
    conn.execute(
        "INSERT INTO events (event_id, event_type, payload_json, subject_guid, signed_by, \
         signature, seq, created_at, warnings_json) VALUES (?1, ?2, ?3, ?4, 'k', 's', ?5, ?6, '[]')",
        rusqlite::params![
            format!("evt-{seq}"),
            event_type,
            payload_json,
            guid,
            seq,
            1_000 + seq
        ],
    )
    .expect("insert event");
}

fn fill(db: &Db) -> usize {
    let mut conn = db.lock().expect("lock db");
    stophammer::db::fill_retired_source_holds(&mut conn).expect("fill the holds")
}

fn hold_of(db: &Db, guid: &str) -> Option<stophammer::db::RetiredSourceHold> {
    let conn = db.lock().expect("lock db");
    stophammer::db::get_retired_source_hold(&conn, guid).expect("read the hold")
}

// Check 7: a retire with no record of the GUID gets a hold with the URL of
// the earlier feed_upserted event.
#[test]
fn the_fill_writes_a_hold_with_the_url_of_the_upserted_event() {
    let db = common::test_db_arc();
    log_upserted(&db, "Fill-A", "https://a.example/feed.xml", 1);
    log_retired(&db, "Fill-A", "podcast_block", 2);
    assert_eq!(fill(&db), 1, "ADR 0079 §4: one hold");
    let hold = hold_of(&db, "fill-a").expect("the hold exists");
    assert_eq!(hold.source_url, "https://a.example/feed.xml");
    assert_eq!(hold.reason, "podcast_block");
    assert_eq!(hold.retired_at, 1_002, "the time of the retire event");
}

#[test]
fn a_second_fill_writes_no_row() {
    let db = common::test_db_arc();
    log_upserted(&db, "fill-b", "https://b.example/feed.xml", 1);
    log_retired(&db, "fill-b", "source_gone", 2);
    assert_eq!(fill(&db), 1, "the first fill writes the hold");
    assert_eq!(fill(&db), 0, "the second fill writes nothing");
    assert_eq!(hold_rows(&db, "fill-b"), 1, "one row stays");
}

#[test]
fn a_guid_with_a_later_upserted_event_gets_no_hold() {
    let db = common::test_db_arc();
    log_upserted(&db, "fill-c", "https://c.example/feed.xml", 1);
    log_retired(&db, "fill-c", "podcast_block", 2);
    {
        let conn = db.lock().expect("lock db");
        conn.execute(
            "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, raw_medium, \
             explicit, episode_count, created_at, updated_at) \
             VALUES ('fill-c', 'https://c.example/feed.xml', 'T', 't', 'music', 0, 0, 1, 1)",
            [],
        )
        .expect("insert feed");
    }
    log_upserted(&db, "fill-c", "https://c.example/feed.xml", 3);
    assert_eq!(fill(&db), 0, "ADR 0079 §4: a record exists, so no hold");
    assert_eq!(hold_rows(&db, "fill-c"), 0, "no hold row");
}

#[test]
fn a_guid_block_gets_no_hold() {
    let db = common::test_db_arc();
    log_upserted(&db, "fill-d", "https://d.example/feed.xml", 1);
    log_retired(&db, "fill-d", "podcast_block", 2);
    {
        let conn = db.lock().expect("lock db");
        conn.execute(
            "INSERT INTO feed_blocks (block_id, kind, value, reason, blocked_at) \
             VALUES ('b1', 'guid', 'fill-d', 'test', 1)",
            [],
        )
        .expect("insert block");
    }
    assert_eq!(fill(&db), 0, "ADR 0079 §4: a GUID block stops the hold");
    assert_eq!(hold_rows(&db, "fill-d"), 0, "no hold row");
}

#[test]
fn a_guid_superseded_retire_gets_no_hold() {
    let db = common::test_db_arc();
    log_upserted(&db, "fill-e", "https://e.example/feed.xml", 1);
    log_retired(&db, "fill-e", "guid_superseded", 2);
    assert_eq!(fill(&db), 0, "ADR 0079 §1: this reason gets no hold");
    assert_eq!(hold_rows(&db, "fill-e"), 0, "no hold row");
}

#[test]
fn a_retire_with_no_earlier_upserted_event_gets_no_hold() {
    let db = common::test_db_arc();
    log_retired(&db, "fill-f", "admin retired via API", 1);
    log_upserted(&db, "fill-f", "https://f.example/feed.xml", 2);
    assert_eq!(fill(&db), 0, "ADR 0079 §4: no earlier URL, so no hold");
    assert_eq!(hold_rows(&db, "fill-f"), 0, "no hold row");
}

#[test]
fn the_hold_has_the_url_of_the_later_upserted_event() {
    let db = common::test_db_arc();
    log_upserted(&db, "fill-g", "https://g.example/old.xml", 1);
    log_upserted(&db, "fill-g", "https://g.example/new.xml", 2);
    log_retired(&db, "fill-g", "admin retired via API", 3);
    assert_eq!(fill(&db), 1, "one hold");
    let hold = hold_of(&db, "fill-g").expect("the hold exists");
    assert_eq!(
        hold.source_url, "https://g.example/new.xml",
        "ADR 0079 §4: the later upserted event gives the URL"
    );
}
