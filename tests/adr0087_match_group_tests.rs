//! ADR 0087: a search orders its hits by groups of fields, and `rank` is the
//! BM25 score.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use stophammer::search::{
    SearchFilters, populate_search_index, search, search_feeds_and_tracks, search_filtered,
};
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0087-groups"));
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

/// Sends `GET /v1/search` with the query pairs and returns the JSON body.
async fn get_search(
    db: &Arc<Mutex<rusqlite::Connection>>,
    pairs: &[(&str, &str)],
) -> serde_json::Value {
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish();
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/search?{query}"))
        .body(axum::body::Body::empty())
        .expect("build request");
    let resp = app(Arc::clone(db))
        .oneshot(req)
        .await
        .expect("call handler");
    assert_eq!(resp.status().as_u16(), 200, "expected a 200 response");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("JSON body")
}

/// The `entity_id` of each row of a body.
fn body_ids(body: &serde_json::Value) -> Vec<String> {
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|row| row["entity_id"].as_str().expect("entity_id").to_owned())
        .collect()
}

/// Writes a feed row with `name`, `title` and `description`.
fn put(conn: &rusqlite::Connection, id: &str, name: &str, title: &str, description: &str) {
    populate_search_index(conn, "feed", id, name, title, description).expect("populate");
}

/// Writes the quality of a feed row.
fn quality(conn: &rusqlite::Connection, id: &str, score: i64) {
    conn.execute(
        "INSERT INTO entity_quality (entity_type, entity_id, score, computed_at) \
         VALUES ('feed', ?1, ?2, 0)",
        rusqlite::params![id, score],
    )
    .expect("insert quality");
}

/// Writes rows with no search word, so that each word has a usable BM25 score.
fn put_other_rows(conn: &rusqlite::Connection) {
    for index in 0..20 {
        put(conn, &format!("f-other-{index}"), "", "Other", "");
    }
}

/// The IDs of a search of the feeds, in the sequence of the result.
fn feed_ids(conn: &rusqlite::Connection, q: &str) -> Vec<String> {
    search(conn, q, Some("feed"), 20, None, None)
        .expect("search")
        .into_iter()
        .map(|hit| hit.entity_id)
        .collect()
}

#[test]
fn a_title_hit_comes_before_a_description_hit() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-text", "", "Callum", "I love music");
    put(
        &conn,
        "f-title",
        "",
        "Strange Love",
        "A very long description with many words that lowers the score of this row in BM25",
    );
    quality(&conn, "f-text", 100);

    assert_eq!(
        feed_ids(&conn, "love"),
        vec!["f-title", "f-text"],
        "ADR 0087 §1: a title hit comes first, also with a longer row and a lower quality"
    );
}

#[test]
fn an_artist_name_hit_has_the_group_of_a_title_hit() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-title", "", "Love", "");
    put(&conn, "f-name", "Love", "Song", "");
    put(&conn, "f-text", "", "Song", "love");

    quality(&conn, "f-name", 100);
    assert_eq!(
        feed_ids(&conn, "love"),
        vec!["f-name", "f-title", "f-text"],
        "ADR 0087 §1: quality can put an artist name hit before a title hit, and never a description hit"
    );
}

#[test]
fn a_title_prefix_hit_comes_before_a_description_hit_with_the_whole_word() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-text", "", "Callum", "I love music");
    put(&conn, "f-prefix", "", "Lover", "");

    assert_eq!(
        feed_ids(&conn, "love"),
        vec!["f-prefix", "f-text"],
        "ADR 0087 §1: a prefix hit in the title comes before a whole word in the description"
    );
}

#[test]
fn a_title_prefix_hit_comes_before_a_whole_word_hit_across_two_fields() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(
        &conn,
        "f-flood",
        "T.E.A.L",
        "THE FLOOD",
        "T.E.A.L - THE FLOOD",
    );
    put(&conn, "f-low", "LIlA", "On The Low", "");
    put(
        &conn,
        "f-whole",
        "Lovenix Music",
        "LVX3 l Into The Unknown l",
        "",
    );
    quality(&conn, "f-flood", 100);

    assert_eq!(
        feed_ids(&conn, "the l"),
        vec!["f-whole", "f-low", "f-flood"],
        "ADR 0087 Invariants: a title prefix hit can come before a whole word hit across two fields"
    );
}

#[test]
fn a_hit_across_two_fields_comes_before_a_description_hit() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-text", "", "Song", "burna life");
    put(&conn, "f-two", "Burna Boy", "Life", "");
    put(&conn, "f-one", "", "Burna Life", "");
    quality(&conn, "f-text", 100);

    assert_eq!(
        feed_ids(&conn, "burna life"),
        vec!["f-one", "f-two", "f-text"],
        "ADR 0087 §1: one field, then two fields, then each other hit"
    );
}

#[test]
fn a_whole_word_comes_before_a_prefix_in_each_group() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-title-prefix", "", "Lover", "");
    put(&conn, "f-title-whole", "", "Love", "");
    put(&conn, "f-text-prefix", "", "Song", "lover");
    put(&conn, "f-text-whole", "", "Song", "love");
    quality(&conn, "f-title-prefix", 100);
    quality(&conn, "f-text-prefix", 100);

    assert_eq!(
        feed_ids(&conn, "love"),
        vec![
            "f-title-whole",
            "f-title-prefix",
            "f-text-whole",
            "f-text-prefix"
        ],
        "ADR 0087 §1: quality does not put a prefix hit before a whole word in its group"
    );
}

#[test]
fn text_in_double_quotes_keeps_the_groups_with_no_prefix() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-text", "", "Song", "love");
    put(&conn, "f-title", "", "Love", "");
    put(&conn, "f-prefix", "", "Lover", "");

    assert_eq!(
        feed_ids(&conn, "\"love\""),
        vec!["f-title", "f-text"],
        "ADR 0087 §1 and ADR 0086 §1: a quoted word has the groups and no prefix hit"
    );
}

#[tokio::test]
async fn rank_is_the_bm25_score_in_each_group_and_each_sort() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_other_rows(&conn);
        put(&conn, "f-title", "", "Love", "");
        put(&conn, "f-prefix", "", "Lover", "");
        put(&conn, "f-text", "", "Song", "love");
        put(&conn, "f-text-prefix", "", "Song", "lover");
    }

    let by_rank = get_search(&db, &[("q", "love"), ("type", "feed")]).await;
    let by_name = get_search(&db, &[("q", "love"), ("type", "feed"), ("sort", "name")]).await;
    let rows = by_rank["data"].as_array().expect("data array");
    assert_eq!(rows.len(), 4, "expected each hit, got {by_rank}");
    for row in rows {
        let rank = row["rank"].as_f64().expect("rank");
        assert!(
            rank <= 0.0,
            "ADR 0087 §2: rank is a BM25 score with no offset, got {row}"
        );
        let same = by_name["data"]
            .as_array()
            .expect("data array")
            .iter()
            .find(|other| other["entity_id"] == row["entity_id"])
            .expect("the same hit");
        assert!(
            (same["rank"].as_f64().expect("rank") - rank).abs() < f64::EPSILON,
            "ADR 0087 §2: a hit has the same rank on each sort, got {row} and {same}"
        );
    }
}

#[tokio::test]
async fn the_pages_cross_the_groups_and_give_each_row_one_time() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_other_rows(&conn);
        put(&conn, "f-title", "", "Love", "");
        put(&conn, "f-prefix", "", "Lover", "");
        put(&conn, "f-text", "", "Song", "love");
        populate_search_index(&conn, "track", "t-name", "Love", "Song", "").expect("populate");
        populate_search_index(&conn, "track", "t-text", "", "Song", "lover").expect("populate");
        populate_search_index(&conn, "publisher", "p-title", "", "Love", "").expect("populate");
    }

    for pairs in [
        vec![("q", "love")],
        vec![("q", "love"), ("type", "feed")],
        vec![("q", "love"), ("type", "track")],
    ] {
        let mut large = pairs.clone();
        large.push(("limit", "50"));
        let expected = body_ids(&get_search(&db, &large).await);
        assert!(!expected.is_empty(), "expected rows for {pairs:?}");

        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut page = pairs.clone();
            page.push(("limit", "1"));
            if let Some(value) = &cursor {
                page.push(("cursor", value));
            }
            let body = get_search(&db, &page).await;
            seen.extend(body_ids(&body));
            cursor = body["pagination"]["cursor"].as_str().map(str::to_owned);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            seen, expected,
            "ADR 0087 §2: the cursor gives each row one time, in the sequence of one large page, for {pairs:?}"
        );
    }
}

#[test]
fn one_statement_gives_the_rows_of_two_merged_searches() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-title", "", "Love", "");
    put(&conn, "f-text", "", "Song", "love");
    populate_search_index(&conn, "track", "t-name", "Love", "Song", "").expect("populate");
    populate_search_index(&conn, "track", "t-text", "", "Lovely", "").expect("populate");
    populate_search_index(&conn, "publisher", "p-title", "", "Love", "").expect("populate");
    let filters = SearchFilters::default();

    for limit in [1, 2, 3, 10] {
        let mut merged = Vec::new();
        for entity_type in ["feed", "track"] {
            merged.extend(
                search_filtered(
                    &conn,
                    "love",
                    &filters,
                    Some(entity_type),
                    limit,
                    None,
                    None,
                )
                .expect("search"),
            );
        }
        merged.sort_by(|a, b| {
            a.effective_rank
                .total_cmp(&b.effective_rank)
                .then_with(|| a.rowid.cmp(&b.rowid))
        });
        merged.truncate(usize::try_from(limit).expect("limit"));
        let merged: Vec<(String, f64)> = merged
            .into_iter()
            .map(|hit| (hit.entity_id, hit.rank))
            .collect();

        let one: Vec<(String, f64)> =
            search_feeds_and_tracks(&conn, "love", &filters, limit, None, None)
                .expect("search")
                .into_iter()
                .map(|hit| (hit.entity_id, hit.rank))
                .collect();
        assert_eq!(
            one, merged,
            "ADR 0087 §3: one statement gives the same rows and ranks, with limit {limit}"
        );
    }
}

#[test]
fn a_typed_search_keeps_the_score_of_a_search_of_each_type() {
    let conn = common::test_db();
    put_other_rows(&conn);
    put(&conn, "f-title", "", "Love", "");
    put(&conn, "f-prefix", "", "Lover", "");
    populate_search_index(&conn, "track", "t-title", "", "Love Song", "").expect("populate");

    let all = search(&conn, "love", None, 20, None, None).expect("search");
    let feeds = search(&conn, "love", Some("feed"), 20, None, None).expect("search");
    assert_eq!(feeds.len(), 2, "expected two feed rows, got {feeds:?}");
    for hit in feeds {
        let same = all
            .iter()
            .find(|other| other.entity_id == hit.entity_id)
            .expect("the same hit");
        assert!(
            (same.rank - hit.rank).abs() < f64::EPSILON,
            "ADR 0087 §3: the type check changes no score, got {hit:?} and {same:?}"
        );
    }
}

#[tokio::test]
async fn a_filter_with_no_type_keeps_the_rows_of_each_type() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        put_other_rows(&conn);
        put(&conn, "f-a", "Ann", "Love Song", "");
        put(&conn, "f-b", "Bob", "Love Dance", "");
        put(&conn, "f-c", "Ann", "Lover", "song");
        for (id, name, title, description) in [
            ("t-a", "Ann", "Love Song", ""),
            ("t-b", "Bob", "Love Dance", ""),
            ("t-c", "Bob", "Song", "love"),
        ] {
            let id = stophammer::db::canonical_track_entity_id("f-a", id);
            populate_search_index(&conn, "track", &id, name, title, description).expect("populate");
        }
        populate_search_index(&conn, "publisher", "p-a", "Ann", "Love Song", "").expect("populate");
    }

    for filter in [
        ("title", "song"),
        ("artist", "ann"),
        ("not_title", "dance"),
        ("not_artist", "bob"),
    ] {
        let mut expected = Vec::new();
        for entity_type in ["feed", "track"] {
            let pairs = [
                ("q", "love"),
                filter,
                ("type", entity_type),
                ("limit", "50"),
            ];
            expected.extend(body_ids(&get_search(&db, &pairs).await));
        }
        expected.sort();
        assert!(!expected.is_empty(), "expected rows for {filter:?}");

        let pairs = [("q", "love"), filter, ("limit", "50")];
        let mut rows = body_ids(&get_search(&db, &pairs).await);
        rows.sort();
        assert_eq!(
            rows, expected,
            "ADR 0087 §3: one statement gives the rows of each type for {filter:?}"
        );
    }
}
