// ADR 0044 task 002: each documented response points at its schema.
//
// docs/tasks/adr-0044-task-002-responses-point-at-schemas.md
//
// The document is read in-process with `openapi::primary_document()`. A test
// must not run `cargo` itself: a nested build waits on the build lock of the
// outer `cargo test`.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// The routes whose body is a type with no schema. Each sends an `Event`,
/// and `Event` does not derive `ToSchema`.
const PLAIN_OBJECT_ROUTES: &[&str] = &["get /sync/events", "post /sync/reconcile"];

const TOKEN: &str = "adr0044-crawl-token";

fn document() -> Value {
    serde_json::to_value(stophammer::openapi::primary_document()).expect("serialize the document")
}

fn before_document() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/openapi-before-adr0044-task-002.json"
    );
    let text = std::fs::read_to_string(path).expect("the fixture must exist");
    serde_json::from_str(&text).expect("the fixture must be JSON")
}

/// Each (key, operation) pair of the document, with the key `method path`.
fn operations(doc: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, op) in item.as_object().expect("path item") {
            if op.get("responses").is_some() {
                out.push((format!("{method} {path}"), op));
            }
        }
    }
    out
}

fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                refs.push(r.clone());
            }
            for v in map.values() {
                collect_refs(v, refs);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_refs(v, refs);
            }
        }
        _ => {}
    }
}

/// The property names of a schema: a `$ref`, or an object with
/// `properties`.
fn properties(doc: &Value, schema: &Value) -> Vec<String> {
    let resolved = match schema.get("$ref").and_then(Value::as_str) {
        Some(r) => {
            let name = r.trim_start_matches("#/components/schemas/");
            &doc["components"]["schemas"][name]
        }
        None => schema,
    };
    resolved["properties"]
        .as_object()
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// The schema of the `data` item of a 200 response: the `$ref` of `data`, or
/// the `$ref` of its array items.
fn data_item_schema<'a>(doc: &'a Value, route: &str) -> &'a Value {
    let (method, path) = route.split_once(' ').expect("method and path");
    let schema =
        &doc["paths"][path][method]["responses"]["200"]["content"]["application/json"]["schema"];
    let data = &schema["properties"]["data"];
    if data.get("items").is_some() {
        &data["items"]
    } else {
        data
    }
}

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0044-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0044-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

async fn call(
    st: &Arc<stophammer::api::AppState>,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> Value {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            req = req.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(&value).expect("serialize"))
        }
        None => Body::empty(),
    };
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(req.body(body).expect("build request"))
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    assert!(status.is_success(), "{method} {uri} failed with {status}");
    serde_json::from_slice(&bytes).expect("the response is JSON")
}

#[test]
fn each_ref_names_a_schema() {
    let doc = document();
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("components.schemas");
    let mut refs = Vec::new();
    collect_refs(&doc["paths"], &mut refs);
    assert!(
        !refs.is_empty(),
        "ADR 0044: the document must hold $ref values"
    );
    for r in refs {
        let name = r.trim_start_matches("#/components/schemas/");
        assert!(
            schemas.contains_key(name),
            "ADR 0044: {r} names no schema in components.schemas. Register the type in response_schemas()."
        );
    }
}

#[test]
fn each_json_response_names_its_fields() {
    let doc = document();
    for (route, op) in operations(&doc) {
        for (status, response) in op["responses"].as_object().expect("responses") {
            let Some(schema) = response
                .get("content")
                .and_then(|c| c.get("application/json"))
                .and_then(|j| j.get("schema"))
            else {
                continue;
            };
            let named = schema.get("$ref").is_some()
                || schema.get("properties").is_some()
                || schema.get("items").and_then(|i| i.get("$ref")).is_some();
            if !named {
                assert!(
                    PLAIN_OBJECT_ROUTES.contains(&route.as_str()),
                    "ADR 0044: {route} {status} has a schema with no field names. Point it at the schema of its type."
                );
            }
        }
    }
}

#[test]
fn each_example_is_unchanged() {
    let doc = document();
    let before = before_document();
    let mut compared = 0;
    for (route, op) in operations(&doc) {
        let (method, path) = route.split_once(' ').expect("method and path");
        for (status, response) in op["responses"].as_object().expect("responses") {
            let now = response
                .get("content")
                .and_then(|c| c.get("application/json"))
                .and_then(|j| j.get("example"));
            let then =
                before["paths"][path][method]["responses"][status]["content"]["application/json"]
                    .get("example");
            if let (Some(now), Some(then)) = (now, then) {
                compared += 1;
                assert_eq!(
                    now, then,
                    "ADR 0044 task 002: the example of {route} {status} changed"
                );
            }
        }
    }
    assert!(compared > 30, "the test compared only {compared} examples");
}

/// Each key of the live `data` value must be a property of the schema that
/// the document gives for the route.
fn assert_keys_in_schema(doc: &Value, route: &str, data: &Value) {
    let item = match data {
        Value::Array(items) => items
            .first()
            .unwrap_or_else(|| panic!("{route} gave no row")),
        other => other,
    };
    let props = properties(doc, data_item_schema(doc, route));
    assert!(
        !props.is_empty(),
        "ADR 0044: the schema of {route} names no field"
    );
    for key in item.as_object().expect("an object").keys() {
        assert!(
            props.contains(key),
            "ADR 0044: {route} sends `{key}`, and its schema does not name it"
        );
    }
}

fn assert_body_keys_in_schema(doc: &Value, route: &str, body: &Value) {
    let (method, path) = route.split_once(' ').expect("method and path");
    let schema =
        &doc["paths"][path][method]["responses"]["200"]["content"]["application/json"]["schema"];
    let props = properties(doc, schema);
    assert!(
        !props.is_empty(),
        "ADR 0044: the schema of {route} names no field"
    );
    for key in body.as_object().expect("an object").keys() {
        assert!(
            props.contains(key),
            "ADR 0044: {route} sends `{key}`, and its schema does not name it"
        );
    }
}

#[tokio::test]
async fn live_responses_match_their_schemas() {
    let doc = document();
    let st = state(common::test_db_arc());
    call(
        &st,
        "POST",
        "/ingest/feed",
        Some(json!({
            "canonical_url": "https://a.example/album.xml",
            "source_url": "https://a.example/album.xml",
            "crawl_token": TOKEN,
            "http_status": 200,
            "content_hash": "hash-adr0044",
            "feed_data": {
                "feed_guid": "album-adr0044",
                "title": "Schema Album",
                "raw_medium": "music",
                "explicit": false,
                "author_name": "Schema Artist",
                "image_url": "https://a.example/cover.jpg",
                "tracks": [{
                    "track_guid": "t-adr0044",
                    "title": "Schema Track",
                    "pub_date": 1_700_000_000,
                    "duration_secs": 200,
                    "explicit": false,
                    "payment_routes": [],
                    "value_time_splits": []
                }]
            }
        })),
    )
    .await;

    let feed = call(&st, "GET", "/v1/feeds/album-adr0044", None).await;
    assert_keys_in_schema(&doc, "get /v1/feeds/{guid}", &feed["data"]);

    let search = call(&st, "GET", "/v1/search?q=Schema", None).await;
    assert_keys_in_schema(&doc, "get /v1/search", &search["data"]);

    let recent = call(&st, "GET", "/v1/feeds/recent", None).await;
    assert_keys_in_schema(&doc, "get /v1/feeds/recent", &recent["data"]);

    let info = call(&st, "GET", "/node/info", None).await;
    assert_body_keys_in_schema(&doc, "get /node/info", &info);

    let caps = call(&st, "GET", "/v1/node/capabilities", None).await;
    assert_body_keys_in_schema(&doc, "get /v1/node/capabilities", &caps);
}
