//! ADR 0080 §1: text between two double quotes in `q` is one phrase.

mod common;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0080-phrase"));
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
fn query_from_words_reads_phrases() {
    for (input, expected) in [
        ("\"night drive\"", "\"night drive\""),
        ("a \"b\"", "\"a\" \"b\""),
        ("a\"b c\"d", "\"a\" \"b c\" \"d\""),
        ("\"unclosed", "\"\"\"unclosed\""),
        ("\"a\" \"b", "\"a\" \"\"\"b\""),
        ("\" x\0y \"", "\"xy\""),
    ] {
        assert_eq!(
            stophammer::search::fts5_query_from_words(input),
            expected,
            "ADR 0080 §1: wrong query for {input:?}"
        );
    }
}

#[test]
fn phrases_list_each_phrase_in_sequence() {
    assert_eq!(
        stophammer::search::fts5_phrases("a \"b  c\" d"),
        vec!["\"a\"", "\"b c\"", "\"d\""],
        "ADR 0080 §1: fts5_phrases gives each phrase in sequence"
    );
}

#[test]
fn phrase_with_the_title_in_sequence_matches() {
    let conn = common::test_db();
    stophammer::search::populate_search_index(&conn, "feed", "f-night", "Night Drive", "", "")
        .expect("populate f-night");
    stophammer::search::populate_search_index(&conn, "feed", "f-drive", "Drive Night", "", "")
        .expect("populate f-drive");

    let ids = |q: &str| -> Vec<String> {
        let mut ids: Vec<String> =
            stophammer::search::search(&conn, q, Some("feed"), 10, None, None)
                .expect("search")
                .into_iter()
                .map(|h| h.entity_id)
                .collect();
        ids.sort();
        ids
    };
    assert_eq!(
        ids("\"night drive\""),
        vec!["f-night"],
        "ADR 0080 check 1: a phrase matches its words in sequence"
    );
    assert_eq!(
        ids("night drive"),
        vec!["f-drive", "f-night"],
        "ADR 0080 check 1: words with no quotes match in any sequence"
    );
}

#[tokio::test]
async fn each_q_with_quotes_gives_200() {
    let db = common::test_db_arc();
    for q in [
        "\"cut",
        "cut\"",
        "a\"b",
        "\"\" cut",
        "\"cut\" \"",
        "'",
        "title:\"cut\"",
    ] {
        let (status, body) = get_search(&db, q).await;
        assert_eq!(
            status, 200,
            "ADR 0080 check 2: q={q:?} must give 200, got {body}"
        );
    }
}

#[tokio::test]
async fn q_with_no_word_gives_400() {
    let db = common::test_db_arc();
    for q in ["\"\"", "\"  \""] {
        let (status, body) = get_search(&db, q).await;
        assert_eq!(status, 400, "ADR 0080 §1: q={q:?} has no word");
        assert!(
            body.contains("q has no word"),
            "ADR 0080 §1: the message must be `q has no word`, got {body}"
        );
    }
}

#[test]
fn openapi_q_description_names_double_quotes() {
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
        text.contains("double quotes"),
        "ADR 0080 §1: the q description must say double quotes, got {text}"
    );
}
