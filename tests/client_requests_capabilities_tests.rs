// Client requests task 001: capabilities and limits.
//
// docs/tasks/client-requests-task-001-capabilities-and-limits.md
//
// Three checks:
// - `GET /v1/node/capabilities` lists the includes each route accepts, and
//   each listed include gives its key in the route response (v4vmm
//   request 3).
// - Each `limit` parameter in the generated OpenAPI document states the
//   maximum the code enforces for its route (musicindex request 4).
// - `GET /v1/publishers` gives a correct `has_more` (musicindex request 4).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn test_app_state(
    db: Arc<Mutex<rusqlite::Connection>>,
    crawl_token: &str,
) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer(
        "test-client-requests-capabilities-signer",
    ));
    let pubkey = signer.pubkey_hex().to_string();

    let spec = stophammer::verify::ChainSpec {
        names: vec!["content_hash".to_string()],
    };
    let chain = stophammer::verify::build_chain(&spec, crawl_token.to_string());

    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-client-requests-capabilities-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

fn ingest_payload(
    canonical_url: &str,
    source_url: &str,
    crawl_token: &str,
    content_hash: &str,
    feed_data: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "canonical_url": canonical_url,
        "source_url": source_url,
        "crawl_token": crawl_token,
        "http_status": 200,
        "content_hash": content_hash,
        "feed_data": feed_data,
    })
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn get(app: axum::Router, uri: &str) -> (http::StatusCode, serde_json::Value) {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    (status, body_json(resp).await)
}

async fn ingest(app: axum::Router, payload: &serde_json::Value) {
    let resp = app
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
    let body = body_json(resp).await;
    assert_eq!(
        body["accepted"].as_bool(),
        Some(true),
        "ingest must be accepted: {body:?}"
    );
}

// ── Item 1: the capabilities route gives the accepted includes ──────────────
// v4vmm request 3.

/// For each include name `GET /v1/node/capabilities` lists for `feed`, a feed
/// read with that include gives the include's key in the response. The same
/// for `track`. `FEED_INCLUDES` and `TRACK_INCLUDES` in `src/query.rs` and
/// `handle_capabilities` must list the same names the route parsing accepts,
/// or this test fails.
#[tokio::test]
async fn each_capability_include_gives_its_key_in_the_response() {
    let crawl_token = "client-requests-capabilities-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    let feed_guid = "cap-includes-feed";
    let feed_url = "https://example.com/cap-includes-feed.xml";
    let track_guid = "cap-includes-track";
    let feed_data = serde_json::json!({
        "feed_guid": feed_guid,
        "title": "Capabilities Include Test Feed",
        "raw_medium": "music",
        "explicit": false,
        "tracks": [{
            "track_guid": track_guid,
            "title": "Capabilities Include Test Track",
            "explicit": false
        }]
    });
    ingest(
        app.clone(),
        &ingest_payload(
            feed_url,
            feed_url,
            crawl_token,
            "cap-includes-hash",
            &feed_data,
        ),
    )
    .await;

    let (status, caps) = get(app.clone(), "/v1/node/capabilities").await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "GET /v1/node/capabilities must answer 200"
    );

    let feed_includes: Vec<String> = caps["include_params"]["feed"]
        .as_array()
        .expect("feed include params must be an array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("each feed include name must be a string")
                .to_string()
        })
        .collect();
    let track_includes: Vec<String> = caps["include_params"]["track"]
        .as_array()
        .expect("track include params must be an array")
        .iter()
        .map(|v| {
            v.as_str()
                .expect("each track include name must be a string")
                .to_string()
        })
        .collect();

    assert!(
        track_includes.iter().any(|name| name == "remote_items"),
        "v4vmm request 3: TRACK_INCLUDES in src/query.rs must list remote_items"
    );
    assert!(
        track_includes.iter().any(|name| name == "publisher"),
        "v4vmm request 3: TRACK_INCLUDES in src/query.rs must list publisher"
    );

    for name in &feed_includes {
        let uri = format!("/v1/feeds/{feed_guid}?include={name}");
        let (status, body) = get(app.clone(), &uri).await;
        assert_eq!(status, http::StatusCode::OK, "GET {uri} must answer 200");
        assert!(
            body["data"].get(name.as_str()).is_some(),
            "v4vmm request 3: FEED_INCLUDES in src/query.rs lists {name}, \
             but GET {uri} gives no {name} key"
        );
    }

    for name in &track_includes {
        let uri = format!("/v1/tracks/{track_guid}?include={name}");
        let (status, body) = get(app.clone(), &uri).await;
        assert_eq!(status, http::StatusCode::OK, "GET {uri} must answer 200");
        assert!(
            body["data"].get(name.as_str()).is_some(),
            "v4vmm request 3: TRACK_INCLUDES in src/query.rs lists {name}, \
             but GET {uri} gives no {name} key"
        );
    }
}

// ── Item 2: the contract gives the maximum of each limit ────────────────────
// musicindex request 4.

/// Each `limit` parameter of the generated `OpenAPI` document has `minimum: 1`
/// and the `maximum` the code enforces for its route:
/// `query::LIST_LIMIT_MAX` (200) for a route that reads `ListQuery`,
/// `PublisherDetailQuery` or `ArtistTracksQuery`, and
/// `query::SEARCH_LIMIT_MAX` (100) for `/v1/search`, `/v1/publishers`,
/// `/v1/copies` and `/v1/guid-changes`.
#[test]
fn each_limit_parameter_states_its_route_maximum() {
    let doc = stophammer::openapi::primary_document();
    let value = serde_json::to_value(&doc).expect("serialize OpenAPI document");

    let expected: &[(&str, i64)] = &[
        ("/v1/feeds/recent", 200),
        ("/v1/tracks", 200),
        ("/v1/publishers/{publisher}", 200),
        ("/v1/feeds/{guid}", 200),
        ("/v1/tracks/{guid}", 200),
        ("/v1/feeds/{guid}/tracks/{track_guid}", 200),
        ("/v1/copies", 100),
        ("/v1/guid-changes", 100),
        ("/v1/search", 100),
        ("/v1/publishers", 100),
    ];

    for (path, maximum) in expected {
        let parameters = value["paths"][path]["get"]["parameters"]
            .as_array()
            .unwrap_or_else(|| {
                panic!("musicindex request 4: GET {path} must list parameters in the document")
            });
        let limit_param = parameters
            .iter()
            .find(|p| p["name"] == "limit")
            .unwrap_or_else(|| {
                panic!("musicindex request 4: GET {path} must have a limit parameter")
            });
        let schema = &limit_param["schema"];
        assert_eq!(
            schema["minimum"].as_i64(),
            Some(1),
            "musicindex request 4: the limit of GET {path} must have minimum 1"
        );
        assert_eq!(
            schema["maximum"].as_i64(),
            Some(*maximum),
            "musicindex request 4: the limit of GET {path} must have maximum {maximum}, \
             matching the constant in src/query.rs"
        );
    }
}

// ── Item 2: /v1/publishers gives a correct has_more ──────────────────────────
// musicindex request 4.

/// With 3 publishers in the database, `limit=2` gives 2 rows and
/// `has_more: true`. `limit=3` gives 3 rows and `has_more: false`.
#[tokio::test]
async fn publishers_route_gives_correct_has_more() {
    let crawl_token = "client-requests-publishers-has-more-token";
    let db = common::test_db_arc();
    let state = test_app_state(Arc::clone(&db), crawl_token);
    let app = stophammer::api::build_router(state);

    for (n, owner) in [
        ("one", "Has More Publisher One"),
        ("two", "Has More Publisher Two"),
        ("three", "Has More Publisher Three"),
    ] {
        let feed_guid = format!("has-more-publisher-feed-{n}");
        let feed_url = format!("https://example.com/has-more-publisher-{n}.xml");
        let feed_data = serde_json::json!({
            "feed_guid": feed_guid,
            "title": format!("Has More Publisher Feed {n}"),
            "raw_medium": "music",
            "owner_name": owner,
            "explicit": false
        });
        ingest(
            app.clone(),
            &ingest_payload(
                &feed_url,
                &feed_url,
                crawl_token,
                &format!("has-more-hash-{n}"),
                &feed_data,
            ),
        )
        .await;
    }

    let (status, body) = get(app.clone(), "/v1/publishers?limit=2").await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "GET /v1/publishers?limit=2 must answer 200"
    );
    assert_eq!(
        body["data"]
            .as_array()
            .expect("data must be an array")
            .len(),
        2,
        "musicindex request 4: limit=2 with 3 publishers must give 2 rows"
    );
    assert_eq!(
        body["pagination"]["has_more"],
        serde_json::json!(true),
        "musicindex request 4: limit=2 with 3 publishers must give has_more: true"
    );

    let (status, body) = get(app.clone(), "/v1/publishers?limit=3").await;
    assert_eq!(
        status,
        http::StatusCode::OK,
        "GET /v1/publishers?limit=3 must answer 200"
    );
    assert_eq!(
        body["data"]
            .as_array()
            .expect("data must be an array")
            .len(),
        3,
        "musicindex request 4: limit=3 with 3 publishers must give 3 rows"
    );
    assert_eq!(
        body["pagination"]["has_more"],
        serde_json::json!(false),
        "musicindex request 4: limit=3 with 3 publishers must give has_more: false"
    );
}
