// ADR 0083 sections 2 to 6: a move by tag or by self link refuses a loop,
// writes the two declaration columns, and keeps the left URLs. Each test
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
    assert_eq!(total, 0, "a loop of two tags must make no move");
    assert_eq!(b.feed_url(GUID), UA, "the record must stay at A");
}

#[tokio::test]
async fn s3b_two_tags_with_one_hash_for_each_url() {
    let b = Bench::new();
    let mut total = 0;
    for _ in 0..4 {
        total += moves(&b.sub(UA, GUID, Some(UB), None, Some("ha")).await);
        total += moves(&b.sub(UB, GUID, Some(UA), None, Some("hb")).await);
    }
    assert_eq!(total, 0, "a loop of two tags must make no move");
    assert_eq!(b.feed_url(GUID), UA, "the record must stay at A");
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
    assert_eq!(total, 1, "the loop must make one move");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
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
    assert!(
        b.left_urls(GUID).is_empty(),
        "a relocation must delete the left URLs"
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
    b.sub(UA, GUID, Some(UB), None, None).await;
    let resp = b.sub(UB, GUID, Some(UA), None, None).await;
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert_eq!(resp["accepted"], false, "{resp:?}");
    let expected = format!("move to {UB} refused: the body names {UA} (ADR 0083)");
    assert!(
        resp["warnings"]
            .as_array()
            .expect("warnings must be an array")
            .iter()
            .any(|w| w.as_str() == Some(expected.as_str())),
        "the refusal must give the warning: {resp:?}"
    );
    assert_eq!(
        b.copy_rows(GUID, UB),
        1,
        "the refusal must write a copy row"
    );
}

#[tokio::test]
async fn a_self_link_refusal_names_the_self_link_value() {
    let b = Bench::new();
    run(&b, &[(UA, None, Some(UB)), (UB, None, Some(UA))]).await;
    let resp = b.sub(UA, GUID, None, Some(UB), None).await;
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    let expected = format!("move to {UA} refused: the body names {UB} (ADR 0083)");
    assert!(
        resp["warnings"]
            .as_array()
            .expect("warnings must be an array")
            .iter()
            .any(|w| w.as_str() == Some(expected.as_str())),
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

#[tokio::test]
async fn a_return_is_refused_when_its_self_link_names_a_held_url() {
    let b = Bench::new();
    // A moves to B. B then names A by tag, as an update at B.
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
    // The body at A names no own URL by tag, and its self link names B.
    let resp = b.sub(UA, GUID, None, Some(UB), None).await;
    assert_eq!(moves(&resp), 0, "the return must not move: {resp:?}");
    assert_eq!(resp["reason"], "source_conflict", "{resp:?}");
    assert_eq!(b.feed_url(GUID), UB, "the record must stay at B");
}

#[tokio::test]
async fn a_return_moves_when_its_tag_names_its_own_url() {
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
    let resp = b.sub(UA, GUID, Some(UA), Some(UB), None).await;
    assert_eq!(moves(&resp), 1, "the return must move: {resp:?}");
    assert_eq!(b.feed_url(GUID), UA, "the record must return to A");
}
