//! ADR 0080 §4 and §6: `GET /v1/search` with filters and no word in `q`.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0080-no-q"));
    let pubkey = signer.pubkey_hex().to_string();
    stophammer::api::build_router(Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(stophammer::verify::VerifierChain::new(
            "test-token".into(),
            vec![],
        )),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    }))
}

/// Sends `GET <uri>` and returns the status and the JSON body.
async fn get(db: &Arc<Mutex<rusqlite::Connection>>, uri: &str) -> (u16, serde_json::Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(axum::body::Body::empty())
        .expect("build request");
    let resp = app(Arc::clone(db))
        .oneshot(req)
        .await
        .expect("call handler");
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
    });
    (status, body)
}

/// The `entity_id` values of a `200` response, in response order.
async fn ids(db: &Arc<Mutex<rusqlite::Connection>>, uri: &str) -> Vec<String> {
    let (status, body) = get(db, uri).await;
    assert_eq!(status, 200, "{uri} must give 200, got {body}");
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|row| row["entity_id"].as_str().expect("entity_id").to_owned())
        .collect()
}

/// A feed id, its host, its date and the index type (`None`: not indexed).
type SeedRow = (
    &'static str,
    &'static str,
    Option<i64>,
    Option<&'static str>,
);

/// The rows of the tests. Each track has the date of its feed. `fd` and `fe`
/// have no date. `fb` and `fc` have the same date. `fn` has no medium, so the
/// index holds no row for it.
///
/// | Feed | Host | Date | Index type |
/// |---|---|---|---|
/// | `fa` | `a.com` | 300 | feed |
/// | `fb` | `x.com` | 200 | feed |
/// | `fc` | `x.com` | 200 | feed |
/// | `fd` | `a.com` | none | feed |
/// | `fe` | `a.com` | none | feed |
/// | `ff` | `a.com` | 100 | feed |
/// | `px` | `x.com` | none | publisher |
/// | `fn` | `x.com` | 400 | none |
fn seeded() -> Arc<Mutex<rusqlite::Connection>> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let rows: [SeedRow; 8] = [
            ("fa", "a.com", Some(300), Some("feed")),
            ("fb", "x.com", Some(200), Some("feed")),
            ("fc", "x.com", Some(200), Some("feed")),
            ("fd", "a.com", None, Some("feed")),
            ("fe", "a.com", None, Some("feed")),
            ("ff", "a.com", Some(100), Some("feed")),
            ("px", "x.com", None, Some("publisher")),
            ("fn", "x.com", Some(400), None),
        ];
        for (feed, host, date, indexed) in rows {
            let track = format!("t{feed}");
            conn.execute(
                "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, feed_host, \
                 newest_item_at, created_at, updated_at) VALUES (?1, ?2, ?1, ?1, ?3, ?4, 1, 1)",
                rusqlite::params![feed, format!("https://{host}/{feed}"), host, date],
            )
            .expect("insert feed");
            conn.execute(
                "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, pub_date, \
                 duration_secs, created_at, updated_at) VALUES (?1, ?2, ?1, ?1, ?3, 10, 1, 1)",
                rusqlite::params![track, feed, date],
            )
            .expect("insert track");
            if let Some(kind) = indexed {
                stophammer::search::populate_search_index(&conn, kind, feed, feed, feed, "common")
                    .expect("populate");
                if kind == "feed" {
                    stophammer::search::populate_search_index(
                        &conn,
                        "track",
                        &stophammer::db::canonical_track_entity_id(feed, &track),
                        feed,
                        &track,
                        "common",
                    )
                    .expect("populate track");
                }
            }
        }
    }
    db
}

/// Follows the cursor with the given `limit` and returns each `entity_id`.
async fn paged(db: &Arc<Mutex<rusqlite::Connection>>, base: &str, limit: usize) -> Vec<String> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..50 {
        let mut uri = format!("{base}&limit={limit}");
        if let Some(c) = &cursor {
            uri.push_str("&cursor=");
            uri.push_str(c);
        }
        let (status, body) = get(db, &uri).await;
        assert_eq!(status, 200, "{uri} must give 200, got {body}");
        all.extend(
            body["data"]
                .as_array()
                .expect("data array")
                .iter()
                .map(|row| row["entity_id"].as_str().expect("entity_id").to_owned()),
        );
        match body["pagination"]["cursor"].as_str() {
            Some(next) if body["pagination"]["has_more"] == true => cursor = Some(next.to_owned()),
            _ => return all,
        }
    }
    panic!("the cursor did not end");
}

#[tokio::test]
async fn feeds_come_newest_first_with_undated_feeds_last() {
    let db = seeded();
    let got = ids(&db, "/v1/search?type=feed&kind=unknown").await;
    // Rows of the same date, and rows with no date, come in rowid order.
    let rowid = |id: &str| stophammer::search::rowid_for("feed", id);
    let mut same_date = vec!["fb", "fc"];
    same_date.sort_by_key(|id| rowid(id));
    let mut undated = vec!["fd", "fe"];
    undated.sort_by_key(|id| rowid(id));
    let mut expected = vec!["fa"];
    expected.extend(same_date);
    expected.push("ff");
    expected.extend(undated);
    assert_eq!(
        got, expected,
        "ADR 0080 check 13: newest first, no date last"
    );
}

#[tokio::test]
async fn the_pages_of_the_cursor_give_each_row_one_time() {
    let db = seeded();
    let whole = ids(&db, "/v1/search?type=feed&kind=unknown").await;
    for limit in [1, 2, 3] {
        let got = paged(&db, "/v1/search?type=feed&kind=unknown", limit).await;
        assert_eq!(
            got, whole,
            "ADR 0080 check 13: limit={limit} gives each row one time"
        );
    }
    let whole = ids(&db, "/v1/search?kind=unknown").await;
    let got = paged(&db, "/v1/search?kind=unknown", 2).await;
    assert_eq!(got, whole, "pages with no type give each row one time");
}

#[tokio::test]
async fn tracks_come_newest_first_by_pub_date() {
    let db = seeded();
    let got = ids(&db, "/v1/search?type=track&min_duration=1").await;
    assert_eq!(got.len(), 6, "each indexed track is a row: {got:?}");
    // The tracks of one date come in rowid order, so compare each date group.
    let group = |range: std::ops::Range<usize>| {
        let mut part = got[range].to_vec();
        part.sort();
        part
    };
    assert_eq!(got[0], "tfa", "pub_date 300 is first");
    assert_eq!(group(1..3), ["tfb", "tfc"], "pub_date 200 is next");
    assert_eq!(got[3], "tff", "pub_date 100 is next");
    assert_eq!(group(4..6), ["tfd", "tfe"], "tracks with no date are last");
}

#[tokio::test]
async fn no_type_gives_feeds_and_tracks_in_one_list_and_no_publisher() {
    let db = seeded();
    let (status, body) = get(&db, "/v1/search?kind=unknown&limit=100").await;
    assert_eq!(status, 200, "got {body}");
    let rows = body["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 12, "6 feeds and 6 tracks, got {body}");
    assert!(
        rows.iter().all(|r| r["entity_type"] != "publisher"),
        "no type gives no publisher, got {body}"
    );
    // The feed fa and its track have the newest date. They come first.
    let mut first: Vec<(&str, &str)> = rows[..2]
        .iter()
        .map(|r| {
            (
                r["entity_type"].as_str().expect("entity_type"),
                r["entity_id"].as_str().expect("entity_id"),
            )
        })
        .collect();
    first.sort_unstable();
    assert_eq!(
        first,
        [("feed", "fa"), ("track", "tfa")],
        "one list orders feeds and tracks by date, got {body}"
    );
}

#[tokio::test]
async fn a_publisher_is_a_row_of_its_type_only() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?type=publisher&host=x.com").await,
        vec!["px"],
        "the publisher feed is a row of type=publisher"
    );
    assert_eq!(
        ids(&db, "/v1/search?type=feed&host=x.com").await.len(),
        2,
        "type=feed gives the music feeds fb and fc, not the publisher"
    );
    for kind in ["feed", "track", "publisher"] {
        let got = ids(&db, &format!("/v1/search?type={kind}&host=x.com")).await;
        assert!(
            got.iter().all(|id| !id.contains("fn")),
            "a feed with no medium is not a row of type={kind}: {got:?}"
        );
    }
}

#[tokio::test]
async fn each_row_has_rank_zero() {
    let db = seeded();
    let (status, body) = get(&db, "/v1/search?kind=unknown").await;
    assert_eq!(status, 200, "got {body}");
    let rows = body["data"].as_array().expect("data array");
    assert!(!rows.is_empty(), "got {body}");
    assert!(
        rows.iter().all(|r| r["rank"] == 0.0),
        "each row has rank 0, got {body}"
    );
}

#[tokio::test]
async fn a_cursor_of_one_form_gives_400_on_the_other_search() {
    let db = seeded();
    let (_, body) = get(&db, "/v1/search?q=common&type=feed&limit=1").await;
    let q_cursor = body["pagination"]["cursor"].as_str().expect("q cursor");
    let (_, body) = get(&db, "/v1/search?type=feed&kind=unknown&limit=1").await;
    let date_cursor = body["pagination"]["cursor"].as_str().expect("date cursor");

    for (uri, expected) in [
        (
            format!("/v1/search?type=feed&kind=unknown&cursor={q_cursor}"),
            "q cursor on a search with no q",
        ),
        (
            format!("/v1/search?q=common&type=feed&cursor={date_cursor}"),
            "date cursor on a search with q",
        ),
    ] {
        let (status, body) = get(&db, &uri).await;
        assert_eq!(status, 400, "ADR 0080 check 14: {expected}");
        assert!(
            body.to_string().contains("invalid cursor format"),
            "{expected}: got {body}"
        );
    }
}

#[tokio::test]
async fn a_request_with_no_positive_filter_gives_missing_positive_term() {
    let db = seeded();
    for uri in [
        "/v1/search?not_title=x",
        "/v1/search",
        "/v1/search?not_host=x.com",
    ] {
        let (status, body) = get(&db, uri).await;
        assert_eq!(status, 400, "{uri}");
        assert_eq!(body["code"], "missing_positive_term", "{uri}: got {body}");
    }
}

#[tokio::test]
async fn a_text_filter_and_an_exclusion_apply_with_no_q() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?type=feed&title=fa").await,
        vec!["fa"],
        "a text filter alone is a positive filter"
    );
    let got = ids(&db, "/v1/search?type=feed&host=x.com&not_title=fb").await;
    assert_eq!(got, vec!["fc"], "an exclusion applies with no q");
    // A release filter reads the title of the feed of a track.
    let got = ids(&db, "/v1/search?release=fa").await;
    assert_eq!(got.len(), 2, "the feed fa and its track, got {got:?}");
}
