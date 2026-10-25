//! ADR 0080 §2, §3, §4 and §7: the text filters, the exclusions and the error
//! codes of `GET /v1/search`.

mod common;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, RwLock};

use http::Request;
use tower::ServiceExt;

fn app(db: Arc<Mutex<rusqlite::Connection>>) -> axum::Router {
    let signer = Arc::new(common::temp_signer("test-adr0080-text-filter"));
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

/// The index rows of the tests. Each row has the word `common` in its
/// description, so `q=common` matches each row.
///
/// | Row | Type | Name | Title |
/// |---|---|---|---|
/// | `f1` | feed | jSmovie | Night Drive |
/// | `f2` | feed | Other Band | Drive Night |
/// | `f4` | feed | Third | Night Walk |
/// | `t1` | track of `f1` | jSmovie | Cut Night |
/// | `t2` | track of `f2` | Other Band | Cut |
/// | `p1` | publisher | | Pub Night |
/// | `p2` | publisher | | Quiet |
fn seeded() -> Arc<Mutex<rusqlite::Connection>> {
    let db = common::test_db_arc();
    {
        let conn = db.lock().expect("lock");
        let put = |kind: &str, id: &str, name: &str, title: &str| {
            stophammer::search::populate_search_index(&conn, kind, id, name, title, "common")
                .expect("populate");
        };
        put("feed", "f1", "jSmovie", "Night Drive");
        put("feed", "f2", "Other Band", "Drive Night");
        put("feed", "f4", "Third", "Night Walk");
        let track =
            |feed: &str, track: &str| stophammer::db::canonical_track_entity_id(feed, track);
        put("track", &track("f1", "t1"), "jSmovie", "Cut Night");
        put("track", &track("f2", "t2"), "Other Band", "Cut");
        put("publisher", "p1", "", "Pub Night");
        put("publisher", "p2", "", "Quiet");
    }
    db
}

/// The sorted `entity_id` values of a `200` response.
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

#[tokio::test]
async fn each_text_filter_matches_one_row_and_not_another() {
    let db = seeded();
    // (uri, expected ids): one row matches and the other does not.
    for (uri, expected) in [
        ("/v1/search?q=common&type=feed&artist=jsmovie", vec!["f1"]),
        (
            "/v1/search?q=common&type=feed&title=%22drive+night%22",
            vec!["f2"],
        ),
        (
            "/v1/search?q=common&type=feed&release=%22drive+night%22",
            vec!["f2"],
        ),
        ("/v1/search?q=common&type=track&artist=jsmovie", vec!["t1"]),
        ("/v1/search?q=common&type=track&title=night", vec!["t1"]),
        (
            "/v1/search?q=common&type=track&release=%22night+drive%22",
            vec!["t1"],
        ),
        ("/v1/search?q=common&type=publisher&artist=pub", vec!["p1"]),
        ("/v1/search?q=common&type=publisher&title=quiet", vec!["p2"]),
    ] {
        assert_eq!(
            ids(&db, uri).await,
            expected,
            "ADR 0080 check 3: wrong rows for {uri}"
        );
    }
}

#[tokio::test]
async fn text_filter_matches_whole_words_and_phrases_in_sequence() {
    let db = seeded();
    assert!(
        ids(&db, "/v1/search?q=common&type=feed&artist=jsmo")
            .await
            .is_empty(),
        "ADR 0080 §2: artist=jsmo must not match jSmovie"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&artist=jsmovie").await,
        vec!["f1"],
        "ADR 0080 §2: artist=jsmovie must match jSmovie"
    );
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&title=%22night+drive%22").await,
        vec!["f1"],
        "ADR 0080 §2: a phrase matches Night Drive and not Drive Night"
    );
}

#[tokio::test]
async fn no_type_merges_feeds_and_tracks_for_a_filter() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&artist=jsmovie").await,
        vec!["f1", "t1"],
        "ADR 0080 §2: a request with no type applies the filter to feeds and tracks"
    );
}

#[tokio::test]
async fn a_filter_does_not_change_the_rank() {
    let db = seeded();
    for kind in ["feed", "track"] {
        let rank_of = |body: &serde_json::Value, id: &str| -> f64 {
            body["data"]
                .as_array()
                .expect("data")
                .iter()
                .find(|row| row["entity_id"] == id)
                .unwrap_or_else(|| panic!("row {id} in {body}"))["rank"]
                .as_f64()
                .expect("rank")
        };
        let id = if kind == "feed" { "f1" } else { "t1" };
        let (_, without) = get(&db, &format!("/v1/search?q=common&type={kind}")).await;
        let (_, with) = get(
            &db,
            &format!("/v1/search?q=common&type={kind}&artist=jsmovie"),
        )
        .await;
        assert!(
            (rank_of(&without, id) - rank_of(&with, id)).abs() < f64::EPSILON,
            "ADR 0080 check 6: the rank of a {kind} must not change with a filter"
        );
    }
}

#[tokio::test]
async fn each_exclusion_removes_the_rows_that_match() {
    let db = seeded();
    for (uri, expected) in [
        (
            "/v1/search?q=common&type=feed&not_artist=jsmovie",
            vec!["f2", "f4"],
        ),
        (
            "/v1/search?q=common&type=feed&not_title=walk",
            vec!["f1", "f2"],
        ),
        (
            "/v1/search?q=common&type=feed&not_release=%22night+drive%22",
            vec!["f2", "f4"],
        ),
        (
            "/v1/search?q=common&type=track&not_artist=jsmovie",
            vec!["t2"],
        ),
        (
            "/v1/search?q=common&type=track&not_release=%22night+drive%22",
            vec!["t2"],
        ),
        (
            "/v1/search?q=common&type=publisher&not_title=quiet",
            vec!["p1"],
        ),
        (
            "/v1/search?q=common&type=publisher&not_artist=pub",
            vec!["p2"],
        ),
        (
            "/v1/search?q=common&type=feed&not_q=jsmovie",
            vec!["f2", "f4"],
        ),
        // Two values of one parameter remove the rows of each value.
        (
            "/v1/search?q=common&type=feed&not_title=walk&not_title=%22drive+night%22",
            vec!["f1"],
        ),
    ] {
        assert_eq!(
            ids(&db, uri).await,
            expected,
            "ADR 0080 check 7: wrong rows for {uri}"
        );
    }
}

#[tokio::test]
async fn not_q_with_two_words_removes_only_a_row_with_both() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed&not_q=night+drive").await,
        vec!["f4"],
        "ADR 0080 check 7: not_q removes f1 and f2, which have both words, and keeps f4"
    );
}

#[tokio::test]
async fn the_eleventh_exclusion_gives_invalid_filter() {
    let db = seeded();
    let mut uri = String::from("/v1/search?q=common&type=feed");
    for n in 0..10 {
        write!(uri, "&not_title=x{n}").expect("write to a String");
    }
    let (status, _) = get(&db, &uri).await;
    assert_eq!(status, 200, "ADR 0080 §3: 10 exclusions are allowed");
    uri.push_str("&not_artist=y");
    let (status, body) = get(&db, &uri).await;
    assert_eq!(status, 400, "ADR 0080 check 8: 11 exclusions give 400");
    assert_eq!(body["code"], "invalid_filter", "got {body}");
    assert_eq!(body["filter"], "not_artist", "the 11th name, got {body}");
}

#[tokio::test]
async fn exclusions_with_no_positive_term_give_missing_positive_term() {
    let db = seeded();
    let (status, body) = get(&db, "/v1/search?not_title=x").await;
    assert_eq!(status, 400, "ADR 0080 check 8");
    assert_eq!(body["code"], "missing_positive_term", "got {body}");
    assert!(
        body.get("filter").is_none(),
        "ADR 0080 §4: missing_positive_term has no filter, got {body}"
    );
    let (status, body) = get(&db, "/v1/search?q=").await;
    assert_eq!(status, 400);
    assert_eq!(body["code"], "missing_positive_term", "got {body}");
}

#[tokio::test]
async fn an_unsupported_filter_names_the_filter() {
    let db = seeded();
    for (uri, filter) in [
        ("/v1/search?q=common&type=publisher&release=x", "release"),
        (
            "/v1/search?q=common&type=publisher&not_release=x",
            "not_release",
        ),
        ("/v1/search?q=common&genre=x", "genre"),
        ("/v1/search?q=common&type=feed&genre=x", "genre"),
    ] {
        let (status, body) = get(&db, uri).await;
        assert_eq!(status, 400, "ADR 0080 check 9: {uri}");
        assert_eq!(body["code"], "unsupported_filter", "{uri}: got {body}");
        assert_eq!(body["filter"], filter, "{uri}: got {body}");
    }
}

#[tokio::test]
async fn a_request_with_no_type_refuses_a_filter_that_a_type_lacks() {
    // `release` applies to `feed` and `track`, so it is allowed without a type.
    let db = seeded();
    let (status, _) = get(&db, "/v1/search?q=common&release=night").await;
    assert_eq!(
        status, 200,
        "ADR 0080 §2: feed and track each apply release"
    );
}

#[tokio::test]
async fn an_invalid_filter_value_gives_invalid_filter() {
    let db = seeded();
    for uri in [
        "/v1/search?q=common&artist=a&artist=b",
        "/v1/search?q=common&artist=",
        "/v1/search?q=common&artist=%22%22",
        "/v1/search?q=common&not_title=%22%22",
    ] {
        let (status, body) = get(&db, uri).await;
        assert_eq!(status, 400, "ADR 0080 check 10: {uri}");
        assert_eq!(body["code"], "invalid_filter", "{uri}: got {body}");
        assert!(body["filter"].is_string(), "{uri}: got {body}");
    }
}

#[tokio::test]
async fn the_checks_run_in_sequence() {
    let db = seeded();
    // Check 1 comes before check 2, 3 and 4.
    let (_, body) = get(&db, "/v1/search?artist=a&artist=b&genre=x").await;
    assert_eq!(body["code"], "unsupported_filter", "got {body}");
    assert_eq!(body["filter"], "genre", "got {body}");
    // Check 2 comes before check 3.
    let (_, body) = get(&db, "/v1/search?type=publisher&artist=a&artist=b&release=x").await;
    assert_eq!(body["code"], "unsupported_filter", "got {body}");
    // Check 3 comes before check 4.
    let (_, body) = get(&db, "/v1/search?artist=a&artist=b").await;
    assert_eq!(body["code"], "invalid_filter", "got {body}");
}

#[tokio::test]
async fn a_second_value_of_a_single_parameter_gives_400_with_no_code() {
    let db = seeded();
    for uri in [
        "/v1/search?q=a&q=b",
        "/v1/search?q=common&type=feed&type=track",
        "/v1/search?q=common&limit=1&limit=2",
    ] {
        let (status, body) = get(&db, uri).await;
        assert_eq!(status, 400, "{uri}");
        assert!(body.get("code").is_none(), "{uri}: got {body}");
    }
}

#[tokio::test]
async fn a_request_with_only_old_parameters_keeps_its_rows() {
    let db = seeded();
    assert_eq!(
        ids(&db, "/v1/search?q=common&type=feed").await,
        vec!["f1", "f2", "f4"],
        "a request with no filter keeps its rows"
    );
    let (_, page1) = get(&db, "/v1/search?q=common&type=feed&limit=2").await;
    let cursor = page1["pagination"]["cursor"].as_str().expect("cursor");
    let (status, page2) = get(
        &db,
        &format!("/v1/search?q=common&type=feed&limit=2&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, 200, "the cursor of page 1 must work, got {page2}");
    assert_eq!(page2["data"].as_array().expect("data").len(), 1);
}

#[tokio::test]
async fn contract_and_capabilities_read_the_filter_table() {
    let doc = serde_json::to_value(stophammer::openapi::primary_document()).expect("serialize");
    let params: Vec<&str> = doc["paths"]["/v1/search"]["get"]["parameters"]
        .as_array()
        .expect("parameters")
        .iter()
        .map(|p| p["name"].as_str().expect("name"))
        .collect();
    let db = common::test_db_arc();
    let (status, caps) = get(&db, "/v1/node/capabilities").await;
    assert_eq!(status, 200);
    for filter in stophammer::query::SEARCH_FILTERS {
        assert!(
            params.contains(&filter.name),
            "ADR 0080 check 15: /openapi.json must declare {}",
            filter.name
        );
        for kind in ["feed", "track", "publisher"] {
            let listed = caps["search_filters"][kind]
                .as_array()
                .expect("search_filters array")
                .iter()
                .any(|n| n == filter.name);
            assert_eq!(
                listed,
                filter.applies_to(kind),
                "ADR 0080 check 15: search_filters.{kind} for {}",
                filter.name
            );
        }
    }
    let schema = &doc["paths"]["/v1/search"]["get"]["responses"]["400"]["content"]["application/json"]
        ["schema"]["$ref"];
    assert_eq!(
        schema, "#/components/schemas/SearchFilterErrorBody",
        "ADR 0080 §7: the 400 response has the error body schema"
    );
}
