//! ADR 0086: the last word of `q` matches as a prefix, and a hit with the whole
//! word comes first.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use stophammer::search::{fts5_prefix_query, populate_search_index, search};
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0086-prefix"));
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

/// Sends `GET /v1/search` with the query pairs and returns the status and body.
async fn get_search(
    db: &Arc<Mutex<rusqlite::Connection>>,
    pairs: &[(&str, &str)],
) -> (u16, serde_json::Value) {
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
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, serde_json::from_slice(&bytes).expect("JSON body"))
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

/// The `rank` of the row with `id`.
fn body_rank(body: &serde_json::Value, id: &str) -> f64 {
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .find(|row| row["entity_id"] == id)
        .unwrap_or_else(|| panic!("row {id} is missing from {body}"))["rank"]
        .as_f64()
        .expect("rank")
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
fn prefix_query_marks_only_a_last_word() {
    for (input, expected) in [
        ("jsmo", Some("\"jsmo\"*")),
        ("night dr", Some("\"night\" \"dr\"*")),
        ("jsmo  ", Some("\"jsmo\"*")),
        ("jsmo*", Some("\"jsmo*\"*")),
        ("\"jsmo", Some("\"\"\"jsmo\"*")),
        ("jsmo \"\"", Some("\"jsmo\"*")),
        ("\"night\" dr", Some("\"night\" \"dr\"*")),
        ("\"jsmo\"", None),
        ("night \"dr\"", None),
        ("\"night dr\"", None),
        ("", None),
        ("\"\"", None),
    ] {
        assert_eq!(
            fts5_prefix_query(input).as_deref(),
            expected,
            "ADR 0086 §1: wrong prefix query for {input:?}"
        );
    }
}

#[test]
fn only_the_last_word_matches_as_a_prefix() {
    let conn = common::test_db();
    populate_search_index(&conn, "feed", "f-movie", "jSmovie", "Drive", "").expect("populate");
    populate_search_index(&conn, "feed", "f-whole", "jsmo", "Driver", "").expect("populate");

    let mut ids = feed_ids(&conn, "jsmo");
    ids.sort();
    assert_eq!(
        ids,
        vec!["f-movie", "f-whole"],
        "ADR 0086 §1: q=jsmo must match the start of jSmovie"
    );
    assert_eq!(
        feed_ids(&conn, "jsmo dr"),
        vec!["f-whole"],
        "ADR 0086 §1: a word that is not the last word stays whole"
    );
    assert_eq!(
        feed_ids(&conn, "\"jsmo\""),
        vec!["f-whole"],
        "ADR 0086 §1: text in double quotes never matches as a prefix"
    );
}

#[tokio::test]
async fn each_last_word_gives_200() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        populate_search_index(&conn, "feed", "f-night", "", "Night O'Brien", "").expect("populate");
    }
    for q in ["night -", "night *", "-", "*", "'", "o'bri", "jsmo*", "a\""] {
        let (status, body) = get_search(&db, &[("q", q)]).await;
        assert_eq!(
            status, 200,
            "ADR 0086 §1: q={q:?} must give 200, got {body}"
        );
    }
    let (_, body) = get_search(&db, &[("q", "o'bri")]).await;
    assert_eq!(
        body_ids(&body),
        vec!["f-night"],
        "ADR 0086 §1: a word with two tokens matches the start of its last token"
    );
}

#[test]
fn a_hit_with_the_whole_word_comes_first() {
    let conn = common::test_db();
    populate_search_index(&conn, "feed", "f-short", "", "Lover", "").expect("populate");
    populate_search_index(
        &conn,
        "feed",
        "f-whole",
        "",
        "One long title that also has the word love in it",
        "",
    )
    .expect("populate");

    let hits = search(&conn, "love", Some("feed"), 10, None, None).expect("search");
    let ids: Vec<&str> = hits.iter().map(|hit| hit.entity_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["f-whole", "f-short"],
        "ADR 0086 §2: the whole word comes first, also with longer text"
    );
    assert!(
        hits[0].effective_rank < hits[1].effective_rank,
        "ADR 0087 §2: the effective rank gives the sequence, got {hits:?}"
    );
}

#[test]
fn a_hit_with_the_whole_word_keeps_its_rank() {
    let conn = common::test_db();
    populate_search_index(&conn, "feed", "f-a", "", "Love Song", "").expect("populate");
    populate_search_index(&conn, "feed", "f-b", "", "Love Love Love", "").expect("populate");
    populate_search_index(&conn, "feed", "f-c", "", "Lovely Love", "").expect("populate");
    populate_search_index(&conn, "feed", "f-d", "", "Lover", "").expect("populate");
    populate_search_index(&conn, "feed", "f-e", "", "Other", "").expect("populate");

    let exact = search(&conn, "\"love\"", Some("feed"), 10, None, None).expect("exact search");
    let with_prefix = search(&conn, "love", Some("feed"), 10, None, None).expect("search");
    assert_eq!(exact.len(), 3, "the exact query has three hits");
    assert_eq!(with_prefix.len(), 4, "the prefix also finds Lover");
    for (old, new) in exact.iter().zip(&with_prefix) {
        assert_eq!(
            (&old.entity_id, old.rank.to_bits()),
            (&new.entity_id, new.rank.to_bits()),
            "ADR 0086 §2: a hit with the whole word keeps its rank and its sequence"
        );
    }
}

#[test]
fn quality_orders_the_hits_by_prefix_only() {
    let conn = common::test_db();
    populate_search_index(&conn, "feed", "f-whole", "", "Love", "").expect("populate");
    populate_search_index(&conn, "feed", "f-low", "", "Lovely", "").expect("populate");
    populate_search_index(&conn, "feed", "f-high", "", "Lovely", "").expect("populate");
    populate_search_index(&conn, "feed", "f-other", "", "Other", "").expect("populate");
    populate_search_index(&conn, "feed", "f-other2", "", "Another", "").expect("populate");
    conn.execute(
        "INSERT INTO entity_quality (entity_type, entity_id, score, computed_at) \
         VALUES ('feed', 'f-high', 100, 0)",
        [],
    )
    .expect("insert quality");

    assert_eq!(
        feed_ids(&conn, "love"),
        vec!["f-whole", "f-high", "f-low"],
        "ADR 0086 §2: quality orders the hits by prefix, after the whole word"
    );
}

#[tokio::test]
async fn pages_with_no_type_give_each_row_one_time() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        for (kind, id, title) in [
            ("feed", "f-whole", "Love"),
            ("feed", "f-prefix", "Lovely"),
            ("track", "[\"f-whole\",\"t-whole\"]", "Love Song"),
            ("track", "[\"f-whole\",\"t-prefix\"]", "Lover"),
            ("feed", "f-other", "Other"),
        ] {
            populate_search_index(&conn, kind, id, "", title, "").expect("populate");
        }
    }

    let (_, one_page) = get_search(&db, &[("q", "love"), ("limit", "20")]).await;
    let expected = body_ids(&one_page);
    assert_eq!(expected.len(), 4, "the search has four hits: {one_page}");

    let mut paged = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..=expected.len() {
        let mut pairs = vec![("q", "love"), ("limit", "1")];
        if let Some(cursor) = &cursor {
            pairs.push(("cursor", cursor));
        }
        let (status, body) = get_search(&db, &pairs).await;
        assert_eq!(status, 200, "each page must give 200: {body}");
        paged.extend(body_ids(&body));
        cursor = body["pagination"]["cursor"].as_str().map(str::to_owned);
        if body["pagination"]["has_more"] == false {
            break;
        }
    }
    assert_eq!(
        paged, expected,
        "ADR 0086 §2: the pages give the sequence of one large page"
    );
}

#[tokio::test]
async fn sort_name_gives_the_hits_by_prefix_with_the_same_rank() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        populate_search_index(&conn, "feed", "f-whole", "", "Love", "").expect("populate");
        populate_search_index(&conn, "feed", "f-prefix", "", "Lovely", "").expect("populate");
        populate_search_index(&conn, "feed", "f-other", "", "Other", "").expect("populate");
    }

    let (_, relevance) = get_search(&db, &[("q", "love"), ("type", "feed")]).await;
    let (status, by_name) =
        get_search(&db, &[("q", "love"), ("type", "feed"), ("sort", "name")]).await;
    assert_eq!(status, 200, "sort=name must give 200: {by_name}");
    for id in ["f-whole", "f-prefix"] {
        assert!(
            (body_rank(&relevance, id) - body_rank(&by_name, id)).abs() < f64::EPSILON,
            "ADR 0086 §2: {id} must have the same rank on each sort"
        );
    }
}

#[tokio::test]
async fn a_filter_and_an_exclusion_stay_whole_words() {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        populate_search_index(&conn, "feed", "f-movie", "jSmovie", "Drive", "").expect("populate");
        populate_search_index(&conn, "feed", "f-other", "", "Other", "").expect("populate");
    }

    let (status, body) =
        get_search(&db, &[("q", "drive"), ("type", "feed"), ("artist", "jsmo")]).await;
    assert_eq!(status, 200, "the filter must give 200: {body}");
    assert!(
        body_ids(&body).is_empty(),
        "ADR 0086 §3: artist=jsmo must not match jSmovie, got {body}"
    );

    let (status, body) =
        get_search(&db, &[("q", "drive"), ("type", "feed"), ("not_q", "jsmo")]).await;
    assert_eq!(status, 200, "the exclusion must give 200: {body}");
    assert_eq!(
        body_ids(&body),
        vec!["f-movie"],
        "ADR 0086 §3: not_q=jsmo must not remove jSmovie"
    );
}
