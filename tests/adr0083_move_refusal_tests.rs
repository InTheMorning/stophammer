// ADR 0083 sections 2 to 6: a return to a left URL needs a tag at each end, a
// tag that is not the arrival tag, and no cooldown. A move writes the two
// declaration columns, and keeps the left URLs. Each test
// sends bodies through `POST /ingest/feed`, with a minimal verifier chain
// (content_hash only) as tests/adr0052_move_trigger_tests.rs does.

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token";
const CRAWL_TOKEN: &str = "adr0083-crawl-token";
const GUID: &str = "adr0083-guid";

const UA: &str = "https://loop.example/feed/a";
const UB: &str = "https://loop.example/feed/b";
const UC: &str = "https://loop.example/feed/c";
const UD: &str = "https://loop.example/feed/d";
const UX: &str = "https://loop.example/feed/x";
const UY: &str = "https://loop.example/feed/y";
const UZ: &str = "https://loop.example/feed/z";

struct Bench {
    db: Arc<Mutex<rusqlite::Connection>>,
    state: Arc<stophammer::api::AppState>,
    counter: AtomicU32,
}

impl Bench {
    fn new() -> Self {
        let db = common::test_db_arc();
        let signer = Arc::new(common::temp_signer("test-adr0083-signer"));
        let pubkey = signer.pubkey_hex().to_string();
        let spec = stophammer::verify::ChainSpec {
            names: vec!["content_hash".to_string()],
        };
        let chain = stophammer::verify::build_chain(&spec, CRAWL_TOKEN.to_string());
        let state = Arc::new(stophammer::api::AppState {
            db: stophammer::db_pool::DbPool::from_writer_only(Arc::clone(&db)),
            chain: Arc::new(chain),
            signer,
            node_pubkey_hex: pubkey,
            admin_token: ADMIN_TOKEN.into(),
            sync_token: None,
            push_client: reqwest::Client::new(),
            push_subscribers: Arc::new(RwLock::new(HashMap::new())),
            sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
            source_gone_hosts: Vec::new(),
            skip_ssrf_validation: true,
            artist_name_cache: stophammer::query::ArtistNameCache::default(),
        });
        Self {
            db,
            state,
            counter: AtomicU32::new(0),
        }
    }

    /// Sends a body for `guid` at `url` (source URL and canonical URL). `tag`
    /// is `itunes:new-feed-url`, `link` is the self link. `hash` is a new
    /// value when `None`.
    async fn sub(
        &self,
        url: &str,
        guid: &str,
        tag: Option<&str>,
        link: Option<&str>,
        hash: Option<&str>,
    ) -> serde_json::Value {
        let links = link.map_or_else(
            || serde_json::json!([]),
            |u| {
                serde_json::json!([{
                    "position": 0,
                    "link_type": "self_feed",
                    "url": u,
                    "extraction_path": "feed.atom:link[@rel='self']"
                }])
            },
        );
        let hash = hash.map_or_else(
            || format!("hash-{}", self.counter.fetch_add(1, Ordering::SeqCst)),
            str::to_string,
        );
        let payload = serde_json::json!({
            "canonical_url": url,
            "source_url": url,
            "crawl_token": CRAWL_TOKEN,
            "http_status": 200,
            "content_hash": hash,
            "redirects": [],
            "feed_data": body_data(guid, tag, &links),
        });
        self.post(&payload).await
    }

    async fn post(&self, payload: &serde_json::Value) -> serde_json::Value {
        let resp = stophammer::api::build_router(Arc::clone(&self.state))
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
        serde_json::from_slice(&bytes).expect("parse json")
    }

    async fn relocate(&self, guid: &str, to: &str) {
        let req = Request::builder()
            .method("PATCH")
            .uri(format!("/v1/feeds/{guid}"))
            .header("Content-Type", "application/json")
            .header("X-Admin-Token", ADMIN_TOKEN)
            .body(Body::from(
                serde_json::to_vec(&serde_json::json!({"feed_url": to, "reason": "review"}))
                    .expect("serialize"),
            ))
            .expect("build request");
        let resp = stophammer::api::build_router(Arc::clone(&self.state))
            .oneshot(req)
            .await
            .expect("send request");
        assert_eq!(resp.status(), 204, "the relocation must succeed");
    }

    fn feed_url(&self, guid: &str) -> String {
        let conn = self.db.lock().expect("lock db");
        stophammer::db::get_feed(&conn, guid)
            .expect("query feed")
            .expect("feed row must exist")
            .feed_url
    }

    fn left_urls(&self, guid: &str) -> Vec<String> {
        let conn = self.db.lock().expect("lock db");
        stophammer::db::get_left_urls(&conn, guid).expect("query left urls")
    }

    fn declared(&self, guid: &str) -> (Option<String>, Option<String>) {
        let conn = self.db.lock().expect("lock db");
        (
            stophammer::db::get_declared_new_feed_url(&conn, guid).expect("query tag"),
            stophammer::db::get_declared_self_url(&conn, guid).expect("query self link"),
        )
    }

    fn state(&self, guid: &str) -> stophammer::db::MoveState {
        let conn = self.db.lock().expect("lock db");
        stophammer::db::get_move_state(&conn, guid).expect("query move state")
    }

    /// Moves the time of the last return to `seconds_ago` seconds before now.
    fn set_last_return(&self, guid: &str, seconds_ago: i64) {
        let conn = self.db.lock().expect("lock db");
        conn.execute(
            "UPDATE feed_move_state SET last_return_at = ?2 WHERE feed_guid = ?1",
            rusqlite::params![guid, stophammer::db::unix_now() - seconds_ago],
        )
        .expect("move the cooldown");
    }

    /// Reads `return_wait_until` from `GET /v1/feeds/{guid}` (ADR 0097 §3).
    async fn return_wait_until(&self, guid: &str) -> serde_json::Value {
        let req = Request::builder()
            .method("GET")
            .uri(format!("/v1/feeds/{guid}"))
            .body(Body::empty())
            .expect("build request");
        let resp = stophammer::api::build_router(Arc::clone(&self.state))
            .oneshot(req)
            .await
            .expect("send request");
        assert_eq!(resp.status(), 200, "the feed read must succeed");
        let bytes = resp
            .into_body()
            .collect()
            .await
            .expect("read body")
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        let data = body["data"].as_object().expect("data must be an object");
        assert!(
            data.contains_key("return_wait_until"),
            "the single-feed read must give the field: {body:?}"
        );
        data["return_wait_until"].clone()
    }

    /// Sends a 301 from the stored source URL `from` to `to`.
    async fn redirect(&self, from: &str, to: &str) -> serde_json::Value {
        let payload = serde_json::json!({
            "canonical_url": to,
            "source_url": from,
            "crawl_token": CRAWL_TOKEN,
            "http_status": 200,
            "content_hash": format!("redirect-{}", self.counter.fetch_add(1, Ordering::SeqCst)),
            "redirects": [{ "url": from, "status": 301 }],
            "feed_data": body_data(GUID, None, &serde_json::json!([])),
        });
        self.post(&payload).await
    }

    fn copy_rows(&self, guid: &str, url: &str) -> i64 {
        let conn = self.db.lock().expect("lock db");
        conn.query_row(
            "SELECT COUNT(*) FROM feed_copies WHERE feed_guid = ?1 AND url = ?2",
            rusqlite::params![guid, url],
            |row| row.get(0),
        )
        .expect("count copies")
    }
}

fn body_data(guid: &str, tag: Option<&str>, links: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "feed_guid": guid,
        "title": "Title",
        "raw_medium": "episodic",
        "explicit": false,
        "new_feed_url": tag,
        "links": links,
        "tracks": [{
            "track_guid": format!("{guid}-track-01"),
            "title": "Track One",
            "explicit": false
        }]
    })
}

/// The number of warnings that start with "moved from".
fn moves(resp: &serde_json::Value) -> usize {
    resp["warnings"].as_array().map_or(0, |warnings| {
        warnings
            .iter()
            .filter(|w| w.as_str().is_some_and(|w| w.starts_with("moved from")))
            .count()
    })
}

/// The warning of a refused return or a waiting redirect.
fn refusal_warning(url: &str) -> String {
    format!("move to {url} refused: the record left that URL (ADR 0083)")
}

/// `true` when the response carries `warning`.
fn has_warning(resp: &serde_json::Value, warning: &str) -> bool {
    resp["warnings"]
        .as_array()
        .expect("warnings must be an array")
        .iter()
        .any(|w| w.as_str() == Some(warning))
}

/// Sends each step, and returns the total of moves.
async fn run(b: &Bench, steps: &[(&str, Option<&str>, Option<&str>)]) -> usize {
    let mut total = 0;
    for (url, tag, link) in steps {
        total += moves(&b.sub(url, GUID, *tag, *link, None).await);
    }
    total
}

// ---------------------------------------------------------------------------
// Loops stop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn s3_two_tags_name_each_other() {
    let b = Bench::new();
    let round = [(UA, Some(UB), None), (UB, Some(UA), None)];
    let mut total = 0;
    for _ in 0..4 {
        total += run(&b, &round).await;
    }
    assert_eq!(total, 1, "a loop of two tags must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn s3b_two_tags_with_one_hash_for_each_url() {
    let b = Bench::new();
    let mut total = 0;
    for _ in 0..4 {
        total += moves(&b.sub(UA, GUID, Some(UB), None, Some("ha")).await);
        total += moves(&b.sub(UB, GUID, Some(UA), None, Some("hb")).await);
    }
    assert_eq!(total, 1, "a loop of two tags must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn s10_two_self_links_name_each_other() {
    let b = Bench::new();
    let round = [(UA, None, Some(UB)), (UB, None, Some(UA))];
    let mut total = 0;
    for _ in 0..4 {
        total += run(&b, &round).await;
    }
    assert_eq!(total, 1, "the loop must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn s11_a_tag_at_a_and_a_self_link_at_b() {
    let b = Bench::new();
    let round = [(UA, Some(UB), None), (UB, None, Some(UA))];
    let mut total = 0;
    for _ in 0..4 {
        total += run(&b, &round).await;
    }
    assert_eq!(total, 1, "the loop must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn s12_a_loop_of_three_tags() {
    let b = Bench::new();
    let round = [
        (UA, Some(UB), None),
        (UB, Some(UX), None),
        (UX, Some(UA), None),
    ];
    let mut total = 0;
    for _ in 0..4 {
        total += run(&b, &round).await;
    }
    assert_eq!(total, 2, "the loop must make two moves");
    assert_eq!(b.feed_url(GUID), UX, "the record must stay at X");
}

#[tokio::test]
async fn a_loop_of_three_self_links() {
    let b = Bench::new();
    let round = [
        (UA, None, Some(UB)),
        (UB, None, Some(UX)),
        (UX, None, Some(UA)),
    ];
    let mut total = 0;
    for _ in 0..4 {
        total += run(&b, &round).await;
    }
    assert_eq!(total, 2, "the loop must make two moves");
    assert_eq!(b.feed_url(GUID), UX, "the record must stay at X");
}

// ---------------------------------------------------------------------------
// Chains complete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn s7_the_usual_migration() {
    let b = Bench::new();
    let total = run(&b, &[(UA, Some(UB), None), (UB, Some(UB), None)]).await;
    assert_eq!(total, 1, "the migration must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must move to B");
}

#[tokio::test]
async fn s14_the_new_url_has_no_tag_and_no_self_link() {
    let b = Bench::new();
    let total = run(&b, &[(UA, Some(UB), None), (UB, None, None)]).await;
    assert_eq!(total, 1, "the migration must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must move to B");
}

#[tokio::test]
async fn s15_the_tag_names_the_new_url_and_the_self_link_is_stale() {
    let b = Bench::new();
    let total = run(&b, &[(UA, Some(UB), None), (UB, Some(UB), Some(UA))]).await;
    assert_eq!(total, 1, "the stale self link must not stop the move");
    assert_eq!(b.feed_url(GUID), UB, "the record must move to B");
}

#[tokio::test]
async fn s13_a_chain_to_x() {
    let b = Bench::new();
    let total = run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UX), None),
            (UX, Some(UX), None),
        ],
    )
    .await;
    assert_eq!(total, 2, "the chain must make two moves");
    assert_eq!(b.feed_url(GUID), UX, "the record must end at X");
}

#[tokio::test]
async fn three_self_links_in_a_chain() {
    let b = Bench::new();
    let total = run(
        &b,
        &[
            (UA, None, Some(UB)),
            (UB, None, Some(UX)),
            (UX, None, Some(UX)),
        ],
    )
    .await;
    assert_eq!(total, 2, "the chain must make two moves");
    assert_eq!(b.feed_url(GUID), UX, "the record must end at X");
}

#[tokio::test]
async fn s16_a_chain_of_three_hops() {
    let b = Bench::new();
    let total = run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UC), None),
            (UC, Some(UD), None),
            (UD, Some(UD), None),
        ],
    )
    .await;
    assert_eq!(total, 3, "the chain must make three moves");
    assert_eq!(b.feed_url(GUID), UD, "the record must end at D");
}

#[tokio::test]
async fn s20_a_chain_with_one_hash_for_each_url() {
    let b = Bench::new();
    let mut total = 0;
    total += moves(&b.sub(UA, GUID, Some(UB), None, Some("ha")).await);
    total += moves(&b.sub(UB, GUID, Some(UX), None, Some("hb")).await);
    total += moves(&b.sub(UX, GUID, Some(UX), None, Some("hx")).await);
    assert_eq!(total, 2, "the chain must make two moves");
    assert_eq!(b.feed_url(GUID), UX, "the record must end at X");
}

#[tokio::test]
async fn s21_the_chain_ends_at_the_url_of_a_different_record() {
    let b = Bench::new();
    b.sub(UX, "other-guid", None, None, None).await;
    let total = run(&b, &[(UA, Some(UB), None), (UB, Some(UX), None)]).await;
    assert_eq!(total, 1, "the first move must happen");
    let resp = b.sub(UX, GUID, Some(UX), None, None).await;
    assert_eq!(
        resp["reason"], "record_conflict",
        "the URL of a different record must give record_conflict: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn s23_a_return_to_an_earlier_host() {
    let b = Bench::new();
    let total = run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
            (UA, Some(UA), None),
        ],
    )
    .await;
    assert_eq!(total, 2, "the move and the return must make two moves");
    assert_eq!(b.feed_url(GUID), UA, "the record must return to A");
}

#[tokio::test]
async fn r1_the_destination_names_a_dead_url_then_corrects_itself() {
    let b = Bench::new();
    let total = run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UX), None),
            (UB, Some(UB), None),
            (UB, Some(UB), None),
            (UA, Some(UB), None),
            (UB, Some(UB), None),
        ],
    )
    .await;
    assert_eq!(total, 1, "only the first move must happen");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn r3_an_operator_relocation_during_a_chain() {
    let b = Bench::new();
    let total = run(&b, &[(UA, Some(UB), None), (UB, Some(UX), None)]).await;
    assert_eq!(total, 1, "the first move must happen");
    b.relocate(GUID, UY).await;
    assert_eq!(
        b.left_urls(GUID),
        vec![UA.to_string(), UB.to_string()],
        "a relocation must keep the left URLs and add the URL that it leaves"
    );
    let after = run(&b, &[(UX, Some(UX), None)]).await;
    assert_eq!(after, 0, "X must not move the record after the relocation");
    assert_eq!(b.feed_url(GUID), UY, "the record must stay at Y");
}

// ---------------------------------------------------------------------------
// Declarations, left URLs, redirects and warnings
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_move_writes_the_columns_from_the_body_at_the_new_url() {
    let b = Bench::new();
    run(&b, &[(UA, Some(UB), None), (UB, Some(UX), Some(UB))]).await;
    assert_eq!(b.feed_url(GUID), UB, "the record must move to B");
    assert_eq!(
        b.declared(GUID),
        (Some(UX.to_string()), Some(UB.to_string())),
        "the move must write both columns from the body at B"
    );
    assert_eq!(
        b.left_urls(GUID),
        vec![UA.to_string()],
        "the old URL must be a left URL"
    );
}

#[tokio::test]
async fn a_mirror_body_with_no_move_changes_neither_column() {
    let b = Bench::new();
    b.sub(UA, GUID, Some(UB), Some(UA), None).await;
    let resp = b.sub(UZ, GUID, Some(UY), Some(UZ), None).await;
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert_eq!(
        b.declared(GUID),
        (Some(UB.to_string()), Some(UA.to_string())),
        "a mirror body with no move must write neither column"
    );
}

#[tokio::test]
async fn a_301_moves_the_record_and_the_old_url_is_a_left_url() {
    let b = Bench::new();
    b.sub(UA, GUID, None, None, None).await;
    let payload = serde_json::json!({
        "canonical_url": UB,
        "source_url": UA,
        "crawl_token": CRAWL_TOKEN,
        "http_status": 200,
        "content_hash": "redirect-hash",
        "redirects": [{ "url": UA, "status": 301 }],
        "feed_data": body_data(GUID, Some(UA), &serde_json::json!([])),
    });
    let resp = b.post(&payload).await;
    assert_eq!(
        moves(&resp),
        1,
        "the redirect must move the record: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UB, "the record must move to B");
    assert_eq!(
        b.left_urls(GUID),
        vec![UA.to_string()],
        "the old URL must be a left URL"
    );
}

#[tokio::test]
async fn a_refused_body_gets_source_conflict_the_warning_and_a_copy_row() {
    let b = Bench::new();
    // A names B, B names A: one move. A then sends a body with the tag A.
    run(&b, &[(UA, Some(UB), None), (UB, Some(UA), None)]).await;
    let resp = b.sub(UA, GUID, Some(UA), None, None).await;
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert_eq!(resp["accepted"], false, "{resp:?}");
    assert!(
        has_warning(&resp, &refusal_warning(UA)),
        "the refusal must give the warning: {resp:?}"
    );
    assert_eq!(
        b.copy_rows(GUID, UA),
        1,
        "the refusal must write a copy row"
    );
}

#[tokio::test]
async fn a_self_link_refusal_gives_the_warning() {
    let b = Bench::new();
    run(&b, &[(UA, None, Some(UB)), (UB, None, Some(UA))]).await;
    let resp = b.sub(UA, GUID, None, Some(UB), None).await;
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert!(
        has_warning(&resp, &refusal_warning(UA)),
        "the refusal must give the warning: {resp:?}"
    );
}

#[tokio::test]
async fn a_mirror_body_at_a_url_that_no_value_names_gets_no_warning() {
    let b = Bench::new();
    b.sub(UA, GUID, Some(UB), None, None).await;
    let resp = b.sub(UZ, GUID, Some(UA), Some(UA), None).await;
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert_eq!(
        resp["warnings"].as_array().map(Vec::len),
        Some(0),
        "no trigger must give no warning: {resp:?}"
    );
}

// ---------------------------------------------------------------------------
// Returns
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_stale_self_link_at_b_and_a_body_at_a_with_no_tag_or_a_tag() {
    let b = Bench::new();
    // A moves to B. The body at B carries the tag B and a stale self link A.
    let total = run(&b, &[(UA, Some(UB), None), (UB, Some(UB), Some(UA))]).await;
    assert_eq!(total, 1, "the first move must happen");
    for tag in [None, Some(UA), Some(UB)] {
        let resp = b.sub(UA, GUID, tag, Some(UA), None).await;
        assert_eq!(moves(&resp), 0, "the return must not move: {resp:?}");
        assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
    }
}

#[tokio::test]
async fn a_return_with_a_tag_at_each_end_moves() {
    let b = Bench::new();
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
        ],
    )
    .await;
    assert_eq!(b.feed_url(GUID), UB, "the record must be at B");
    // The body at A names no own URL by tag.
    let resp = b.sub(UA, GUID, None, Some(UB), None).await;
    assert_eq!(moves(&resp), 0, "the return must not move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
    // The body at A names its own URL by tag.
    let resp = b.sub(UA, GUID, Some(UA), Some(UB), None).await;
    assert_eq!(moves(&resp), 1, "the return must move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UA, "the record must return to A");
    assert_eq!(b.state(GUID).return_count, 1, "a return adds one");
}

#[tokio::test]
async fn a_return_with_self_links_only_stays() {
    let b = Bench::new();
    let total = run(
        &b,
        &[
            (UA, None, Some(UB)),
            (UB, None, Some(UA)),
            (UA, None, Some(UA)),
        ],
    )
    .await;
    assert_eq!(total, 1, "only the first move must happen");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn a_copied_tag_does_not_start_a_return_until_it_changes() {
    let b = Bench::new();
    let total = run(&b, &[(UA, Some(UB), None), (UB, Some(UA), None)]).await;
    assert_eq!(total, 1, "the first move must happen");
    assert_eq!(
        b.state(GUID).arrival_tag.as_deref(),
        Some(UA),
        "the move must store the tag of the body at B"
    );
    // The body at A has a tag. The tag at B is the arrival tag, so it is refused.
    let resp = b.sub(UA, GUID, Some(UA), None, None).await;
    assert_eq!(moves(&resp), 0, "a copied tag must not return: {resp:?}");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
    // B changes its tag, and then names A again: a real edit.
    let total = run(&b, &[(UB, Some(UB), None)]).await;
    assert_eq!(total, 0, "an update is not a move");
    assert_eq!(
        b.state(GUID).arrival_tag,
        None,
        "a tag that differs must clear the arrival tag"
    );
    let resp = b.sub(UA, GUID, Some(UA), None, None).await;
    assert_eq!(moves(&resp), 0, "the tag B does not name A: {resp:?}");
    let total = run(&b, &[(UB, Some(UA), None)]).await;
    assert_eq!(total, 0, "an update is not a move");
    let resp = b.sub(UA, GUID, Some(UA), None, None).await;
    assert_eq!(
        moves(&resp),
        1,
        "the changed tag must allow the return: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UA, "the record must return to A");
}

#[tokio::test]
// ADR 0097: the default first wait is 60 seconds, and
// `GET /v1/feeds/{guid}` gives its end in `return_wait_until`.
async fn a_second_return_waits_and_the_feed_read_gives_the_end() {
    let b = Bench::new();
    // A to B, then the first return to A.
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
            (UA, Some(UA), None),
        ],
    )
    .await;
    assert_eq!(b.feed_url(GUID), UA, "the first return must move");
    assert_eq!(b.state(GUID).return_count, 1, "the first return counts");
    let last = b.state(GUID).last_return_at.expect("the return time");
    assert_eq!(
        b.return_wait_until(GUID).await,
        serde_json::json!(last + 60),
        "the field must give the end of the 60-second wait"
    );
    // A names B again, and B names itself. This is a second return.
    run(&b, &[(UA, Some(UB), None)]).await;
    let resp = b.sub(UB, GUID, Some(UB), None, None).await;
    assert_eq!(moves(&resp), 0, "the wait must hold the return: {resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert!(
        has_warning(&resp, &refusal_warning(UB)),
        "the wait must give the warning: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UA, "the record must stay at A");
    // The wait passes.
    b.set_last_return(GUID, 61);
    assert_eq!(
        b.return_wait_until(GUID).await,
        serde_json::Value::Null,
        "the field must be null when no wait is active"
    );
    let resp = b.sub(UB, GUID, Some(UB), None, None).await;
    assert_eq!(
        moves(&resp),
        1,
        "the return must move after the wait: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UB, "the record must return to B");
    assert_eq!(b.state(GUID).return_count, 2, "the second return counts");
}

#[tokio::test]
async fn the_count_starts_again_after_30_days() {
    let b = Bench::new();
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
            (UA, Some(UA), None),
        ],
    )
    .await;
    // Three returns count, but the last one is 31 days old.
    {
        let conn = b.db.lock().expect("lock db");
        conn.execute(
            "UPDATE feed_move_state SET return_count = 3 WHERE feed_guid = ?1",
            rusqlite::params![GUID],
        )
        .expect("set the count");
    }
    b.set_last_return(GUID, 31 * 86_400);
    run(&b, &[(UA, Some(UB), None)]).await;
    let resp = b.sub(UB, GUID, Some(UB), None, None).await;
    assert_eq!(
        moves(&resp),
        1,
        "no wait must apply after 30 days: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UB, "the record must return to B");
    assert_eq!(
        b.state(GUID).return_count,
        1,
        "the count must start again at one"
    );
}

#[tokio::test]
async fn a_move_to_a_url_that_is_not_a_left_url_never_waits() {
    let b = Bench::new();
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
            (UA, Some(UA), None),
        ],
    )
    .await;
    let before = b.state(GUID);
    let total = run(&b, &[(UA, Some(UX), None), (UX, Some(UX), None)]).await;
    assert_eq!(total, 1, "a move to a new URL must not wait");
    assert_eq!(b.feed_url(GUID), UX, "the record must move to X");
    let after = b.state(GUID);
    assert_eq!(
        (before.return_count, before.last_return_at),
        (after.return_count, after.last_return_at),
        "a move to a new URL must change neither the count nor the time"
    );
}

#[tokio::test]
async fn a_redirect_to_a_left_url_waits_and_then_moves() {
    let b = Bench::new();
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
            (UA, Some(UA), None),
        ],
    )
    .await;
    assert_eq!(b.feed_url(GUID), UA, "the first return must move");
    // B is a left URL, and the first return was just now.
    let resp = b.redirect(UA, UB).await;
    assert_eq!(moves(&resp), 0, "the wait must hold the redirect: {resp:?}");
    assert_eq!(resp["accepted"], true, "the content must apply: {resp:?}");
    assert!(
        has_warning(&resp, &refusal_warning(UB)),
        "the wait must give the warning: {resp:?}"
    );
    assert_eq!(b.feed_url(GUID), UA, "the source URL must stay");
    b.set_last_return(GUID, 3601);
    let resp = b.redirect(UA, UB).await;
    assert_eq!(moves(&resp), 1, "the redirect must move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UB, "the record must move to B");
    assert_eq!(b.state(GUID).return_count, 2, "the redirect is a return");
}

#[tokio::test]
async fn after_a_relocation_the_old_url_cannot_start_a_move_before_the_new_source_names_it() {
    let b = Bench::new();
    run(&b, &[(UA, Some(UB), None), (UB, Some(UB), None)]).await;
    assert_eq!(b.feed_url(GUID), UB, "the record must be at B");
    b.relocate(GUID, UY).await;
    assert_eq!(
        b.state(GUID).arrival_tag,
        None,
        "a relocation clears the tag"
    );
    assert_eq!(
        b.state(GUID).return_count,
        0,
        "a relocation zeroes the count"
    );
    // The relocation cleared the declarations: B cannot start a move.
    let resp = b.sub(UB, GUID, Some(UB), None, None).await;
    assert_eq!(moves(&resp), 0, "B must not move the record: {resp:?}");
    assert_eq!(b.feed_url(GUID), UY, "the record must stay at Y");
}

#[tokio::test]
async fn after_a_relocation_a_stale_tag_at_the_new_source_cannot_bring_the_record_back() {
    let b = Bench::new();
    run(&b, &[(UA, Some(UB), None), (UB, Some(UB), None)]).await;
    b.relocate(GUID, UY).await;
    // The first body from Y names B. It can be a stale copy.
    run(&b, &[(UY, Some(UB), None)]).await;
    assert_eq!(
        b.state(GUID).arrival_tag.as_deref(),
        Some(UB),
        "the first body after a relocation must give the arrival tag"
    );
    let resp = b.sub(UB, GUID, Some(UB), None, None).await;
    assert_eq!(moves(&resp), 0, "the stale tag must not move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UY, "the record must stay at Y");
    // Y changes its tag and names B again, so the statement is real.
    run(&b, &[(UY, Some(UY), None), (UY, Some(UB), None)]).await;
    let resp = b.sub(UB, GUID, Some(UB), None, None).await;
    assert_eq!(moves(&resp), 1, "the real statement must move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UB, "the record must be at B");
}

#[tokio::test]
async fn a_move_to_a_new_url_goes_on_when_the_submission_starts_at_a_left_url() {
    let b = Bench::new();
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UC), None),
        ],
    )
    .await;
    assert_eq!(b.feed_url(GUID), UB, "the record must be at B");
    // A is a left URL, and it redirects to C, which the record never left.
    let resp = b
        .post(&serde_json::json!({
            "canonical_url": UC,
            "source_url": UA,
            "crawl_token": CRAWL_TOKEN,
            "http_status": 200,
            "content_hash": "hash-left-source",
            "redirects": [{ "url": UC, "status": 302 }],
            "feed_data": body_data(GUID, Some(UC), &serde_json::json!([])),
        }))
        .await;
    assert_eq!(moves(&resp), 1, "the move to C must go on: {resp:?}");
    assert_eq!(b.feed_url(GUID), UC, "the record must be at C");
}

#[tokio::test]
async fn a_return_needs_a_tag_that_names_the_url_of_the_return() {
    let b = Bench::new();
    run(
        &b,
        &[
            (UA, Some(UB), None),
            (UB, Some(UB), None),
            (UB, Some(UA), None),
        ],
    )
    .await;
    assert_eq!(b.feed_url(GUID), UB, "the record must be at B");
    // C redirects to A, and the body names C, not A.
    let resp = b
        .post(&serde_json::json!({
            "canonical_url": UA,
            "source_url": UC,
            "crawl_token": CRAWL_TOKEN,
            "http_status": 200,
            "content_hash": "hash-tag-names-c",
            "redirects": [{ "url": UA, "status": 302 }],
            "feed_data": body_data(GUID, Some(UC), &serde_json::json!([])),
        }))
        .await;
    assert_eq!(moves(&resp), 0, "the return must not move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}
