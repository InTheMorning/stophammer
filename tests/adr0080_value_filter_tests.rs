//! ADR 0080 §2 and §7: the kind, count, date and duration filters of
//! `GET /v1/search`.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

/// 2026-10-01T00:00:00Z as Unix seconds.
const OCT_1: i64 = 1_790_812_800;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0080-value-filter"));
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

/// The sorted `entity_id` values of a `200` response. A track gives its GUID.
async fn ids(db: &Arc<Mutex<rusqlite::Connection>>, uri: &str) -> Vec<String> {
    let (status, body) = get(db, uri).await;
    assert_eq!(status, 200, "{uri} must give 200, got {body}");
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|row| row["entity_id"].as_str().expect("entity_id").to_owned())
        .collect();
    ids.sort();
    ids
}

/// A feed id, its kind, its track count, its date and its duration.
type SeedRow = (
    &'static str,
    Option<&'static str>,
    i64,
    Option<i64>,
    Option<i64>,
);

/// The rows of the tests. Each indexed row has the word `common`.
///
/// | Feed | Kind | Tracks | Newest item | Track | Date | Duration |
/// |---|---|---|---|---|---|---|
/// | `f1` | album | 1 | Oct 1 00:00:00 | `t1` | Oct 1 00:00:00 | 180 |
/// | `f2` | ep | 3 | Sep 30 23:59:59 | `t2` | Sep 30 23:59:59 | 179 |
/// | `f3` | single | 5 | Oct 1 23:59:59 | `t3` | Oct 1 23:59:59 | none |
/// | `f4` | none | 0 | none | `t4` | none | 181 |
/// | `f5` | unknown | 2 | Oct 2 00:00:00 | `t5` | Oct 2 00:00:00 | 200 |
fn seeded() -> Arc<Mutex<rusqlite::Connection>> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let rows: [SeedRow; 5] = [
            ("f1", Some("album"), 1, Some(OCT_1), Some(180)),
            ("f2", Some("ep"), 3, Some(OCT_1 - 1), Some(179)),
            ("f3", Some("single"), 5, Some(OCT_1 + 86_399), None),
            ("f4", None, 0, None, Some(181)),
            ("f5", Some("unknown"), 2, Some(OCT_1 + 86_400), Some(200)),
        ];
        for (feed, kind, count, date, duration) in rows {
            let track = format!("t{}", &feed[1..]);
            conn.execute(
                "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, release_kind, \
                 episode_count, newest_item_at, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, 1, 1)",
                rusqlite::params![
                    feed,
                    format!("https://example.test/{feed}"),
                    feed,
                    kind,
                    count,
                    date
                ],
            )
            .expect("insert feed");
            conn.execute(
                "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, pub_date, \
                 duration_secs, created_at, updated_at) VALUES (?1, ?2, ?1, ?1, ?3, ?4, 1, 1)",
                rusqlite::params![track, feed, date, duration],
            )
            .expect("insert track");
            stophammer::search::populate_search_index(&conn, "feed", feed, feed, feed, "common")
                .expect("populate feed");
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
    db
}

#[tokio::test]
async fn each_value_filter_matches_one_row_and_not_another() {
    let db = seeded();
    // ADR 0080 check 3: (uri, expected ids). One row matches, another does not.
    for (uri, expected) in [
        ("type=feed&kind=ep", vec!["f2"]),
        ("type=track&kind=ep", vec!["t2"]),
        ("type=feed&tracks=3", vec!["f2"]),
        ("type=track&tracks=3", vec!["t2"]),
        ("type=feed&min_tracks=5", vec!["f3"]),
        ("type=track&min_tracks=5", vec!["t3"]),
        ("type=feed&max_tracks=0", vec!["f4"]),
        ("type=track&max_tracks=0", vec!["t4"]),
        ("type=feed&after=2026-10-02", vec!["f5"]),
        ("type=track&after=2026-10-02", vec!["t5"]),
        ("type=feed&before=2026-09-30", vec!["f2"]),
        ("type=track&before=2026-09-30", vec!["t2"]),
        ("type=track&min_duration=200", vec!["t5"]),
        ("type=track&max_duration=179", vec!["t2"]),
    ] {
        let uri = format!("/v1/search?q=common&{uri}");
        assert_eq!(ids(&db, &uri).await, expected, "{uri}");
    }
}

#[tokio::test]
async fn kind_unknown_matches_unknown_and_no_kind() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&kind=unknown").await,
        vec!["f4", "f5"],
        "ADR 0080 check 4: unknown also matches a feed with no kind"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&kind=album,single").await,
        vec!["f1", "f3"],
        "ADR 0080 check 4: the kinds are an OR and an EP does not match"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=track&kind=album,single").await,
        vec!["t1", "t3"],
        "ADR 0080 check 4: a track matches by the kind of its feed"
    );
}

#[tokio::test]
async fn dates_include_their_limits_in_utc() {
    let db = seeded();
    // `after` matches from 00:00:00, and not one second before.
    assert_eq!(
        ids(
            &db,
            "/v1/search?q=common&type=feed&after=2026-10-01&before=2026-10-01"
        )
        .await,
        vec!["f1", "f3"],
        "ADR 0080 check 5: both limits include their day"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=track&after=2026-10-01").await,
        vec!["t1", "t3", "t5"],
        "ADR 0080 check 5: Sep 30 23:59:59 does not match"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=track&before=2026-10-01").await,
        vec!["t1", "t2", "t3"],
        "ADR 0080 check 5: Oct 2 00:00:00 does not match, a row with no date does not match"
    );
    assert_eq!(
        ids(
            &db,
            "/v1/search?q=common&type=track&after=2026-10-02&before=2026-10-01"
        )
        .await,
        Vec::<String>::new(),
        "an after later than before gives no row"
    );
    assert_eq!(
        ids(
            &db,
            "/v1/search?q=common&type=feed&min_tracks=3&max_tracks=2"
        )
        .await,
        Vec::<String>::new(),
        "a min_tracks above max_tracks gives no row"
    );
}

#[tokio::test]
async fn tracks_one_matches_the_feed_and_its_track() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&tracks=1").await,
        vec!["f1", "t1"]
    );
}

#[tokio::test]
async fn duration_limits_are_included_and_a_null_duration_does_not_match() {
    let db = seeded();
    assert_eq!(
        ids(
            &db,
            "/v1/search?q=common&type=track&min_duration=180&max_duration=180"
        )
        .await,
        vec!["t1"]
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=track&min_duration=0").await,
        vec!["t1", "t2", "t4", "t5"],
        "t3 has no duration"
    );
}

#[tokio::test]
async fn a_value_filter_of_the_wrong_type_gives_unsupported_filter() {
    let db = seeded();
    for (uri, name) in [
        ("type=feed&min_duration=1", "min_duration"),
        ("type=publisher&kind=album", "kind"),
        ("min_duration=1", "min_duration"),
        ("type=publisher&after=2026-10-01", "after"),
    ] {
        let (status, body) = get(&db, &format!("/v1/search?q=common&{uri}")).await;
        assert_eq!(status, 400, "ADR 0080 check 9: {uri}");
        assert_eq!(body["code"], "unsupported_filter", "{uri}: got {body}");
        assert_eq!(body["filter"], name, "{uri}: got {body}");
    }
}

#[tokio::test]
async fn a_value_in_the_wrong_form_gives_invalid_filter() {
    let db = seeded();
    for (uri, name) in [
        ("after=2026-13-01", "after"),
        ("after=2026-02-30", "after"),
        ("after=2026-10", "after"),
        ("after=yesterday", "after"),
        ("before=2026-1-01x", "before"),
        ("tracks=-1", "tracks"),
        ("tracks=1.5", "tracks"),
        ("tracks=%2B1", "tracks"),
        ("min_tracks=99999999999999999999", "min_tracks"),
        ("type=track&min_duration=x", "min_duration"),
        ("kind=lp", "kind"),
        ("kind=album,", "kind"),
        ("kind=", "kind"),
        ("tracks=1&tracks=2", "tracks"),
    ] {
        let (status, body) = get(&db, &format!("/v1/search?q=common&{uri}")).await;
        assert_eq!(status, 400, "ADR 0080 check 10: {uri}");
        assert_eq!(body["code"], "invalid_filter", "{uri}: got {body}");
        assert_eq!(body["filter"], name, "{uri}: got {body}");
    }
}

#[tokio::test]
async fn a_value_filter_does_not_change_the_rank() {
    let db = seeded();
    let (_, plain) = get(&db, "/v1/search?q=common&type=feed&kind=unknown").await;
    let (_, base) = get(&db, "/v1/search?q=common&type=feed").await;
    let rank_of = |body: &serde_json::Value, id: &str| {
        body["data"]
            .as_array()
            .expect("data")
            .iter()
            .find(|r| r["entity_id"] == id)
            .map(|r| r["rank"].clone())
            .expect("row")
    };
    for id in ["f4", "f5"] {
        assert_eq!(
            rank_of(&plain, id),
            rank_of(&base, id),
            "ADR 0080 check 6: rank of {id} must not change"
        );
    }
}
