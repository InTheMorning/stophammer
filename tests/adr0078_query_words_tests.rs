//! ADR 0078 §5: the text of `q` is words.

mod common;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0078-words"));
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

/// Sends `GET /v1/search?q=<encoded>` and returns the status and the body.
async fn get_search(db: &Arc<Mutex<rusqlite::Connection>>, q: &str) -> (u16, String) {
    let mut uri = String::from("/v1/search?q=");
    for byte in q.bytes() {
        write!(uri, "%{byte:02X}").expect("write to a String");
    }
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
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[test]
fn query_from_words_quotes_each_word() {
    assert_eq!(
        stophammer::search::fts5_query_from_words("a \"b\""),
        "\"a\" \"\"\"b\"\"\"",
        "ADR 0078 §5: a double quote in a word is doubled"
    );
    assert_eq!(
        stophammer::search::fts5_query_from_words("  cut   band "),
        "\"cut\" \"band\"",
        "ADR 0078 §5: white space divides words"
    );
}

#[test]
fn query_from_words_removes_control_characters_before_the_division() {
    assert_eq!(
        stophammer::search::fts5_query_from_words("ab\0cd"),
        "\"abcd\"",
        "ADR 0078 §5: a NUL character is removed and does not divide a word"
    );
    assert!(
        stophammer::search::fts5_query_from_words("\0\u{1}\t\n ").is_empty(),
        "ADR 0078 §5: a q of only control characters and white space has no word"
    );
    assert!(stophammer::search::fts5_query_from_words("").is_empty());
}

#[tokio::test]
async fn each_q_with_a_word_gives_200() {
    let db = common::test_db_arc();
    for q in [
        "haleen'",
        "a\"b",
        "\"unclosed",
        "title:foo",
        "artist:foo",
        "cut*",
        "-cut",
        "(cut)",
        "^cut",
        "AND",
        "NEAR",
        "cut OR band",
        "cut\0band",
        "'",
    ] {
        let (status, body) = get_search(&db, q).await;
        assert_eq!(
            status, 200,
            "ADR 0078 §5: q={q:?} must give 200, got {body}"
        );
    }
}

#[test]
fn title_prefix_is_not_a_column_filter() {
    let conn = common::test_db();
    stophammer::search::populate_search_index(
        &conn,
        "feed",
        "f-desc",
        "Alpha",
        "Beta",
        "title foo",
    )
    .expect("populate f-desc");
    stophammer::search::populate_search_index(&conn, "feed", "f-only", "Gamma", "Delta", "foo")
        .expect("populate f-only");

    let hits = stophammer::search::search(&conn, "title:foo", Some("feed"), 10, None, None)
        .expect("search");
    let ids: Vec<&str> = hits.iter().map(|h| h.entity_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["f-desc"],
        "ADR 0078 §5: title:foo reads as the words `title foo`, with no column filter"
    );
}

#[tokio::test]
async fn q_with_no_word_gives_400() {
    let db = common::test_db_arc();
    for q in ["", " \t\n ", "\0", "\u{1}\u{7f}"] {
        let (status, body) = get_search(&db, q).await;
        assert_eq!(status, 400, "ADR 0078 §5: q={q:?} has no word");
        assert!(
            body.contains("q has no word"),
            "ADR 0078 §5: the message must be `q has no word`, got {body}"
        );
    }
}

#[tokio::test]
async fn no_400_body_holds_sqlite_text() {
    let db = common::test_db_arc();
    for q in ["", "\0", "%", "x"] {
        let (status, body) = get_search(&db, q).await;
        if status == 400 {
            let lower = body.to_lowercase();
            assert!(
                !lower.contains("sqlite") && !lower.contains("fts5"),
                "ADR 0078 §5: a 400 body must hold no SQLite text, got {body}"
            );
        }
    }
    let req = Request::builder()
        .method("GET")
        .uri("/v1/search?q=cut&type=nope")
        .body(axum::body::Body::empty())
        .expect("build request");
    let resp = app(Arc::clone(&db))
        .oneshot(req)
        .await
        .expect("call handler");
    assert_eq!(resp.status().as_u16(), 400);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let body = String::from_utf8_lossy(&bytes).to_lowercase();
    assert!(
        !body.contains("sqlite") && !body.contains("fts5"),
        "ADR 0078 §5: a 400 body must hold no SQLite text, got {body}"
    );
}

#[test]
fn openapi_q_description_is_words() {
    let doc = serde_json::to_value(stophammer::openapi::primary_document()).expect("serialize");
    let params = doc["paths"]["/v1/search"]["get"]["parameters"]
        .as_array()
        .expect("parameters");
    let q = params
        .iter()
        .find(|p| p["name"] == "q")
        .expect("the q parameter");
    let text = q["description"].as_str().expect("description");
    assert!(
        !text.contains("FTS5 syntax"),
        "ADR 0078 §5: the q description must not say FTS5 syntax, got {text}"
    );
}
