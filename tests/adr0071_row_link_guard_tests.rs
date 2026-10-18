// ADR 0071 task 001: the guards of rules 1a and 1c, and the feed GUID of a
// release row.
//
// docs/tasks/adr-0071-task-001-row-link-guards.md
//
// Rule 1a reads the schemas of the OpenAPI document. Rule 1c reads the
// document and the source of `src/query.rs`: axum gives no list of the
// query parameters that a handler reads, so the test finds each `.includes(`
// call in the source and names the function that holds it.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// Each pair of a feed title and its GUID whose names do not share a prefix:
/// (schema, title property, GUID property). ADR 0071 §1a.
const TITLE_GUID_PAIRS: &[(&str, &str, &str)] = &[
    (
        "CopyRemoteItemResponse",
        "remote_feed_title",
        "resolved_feed_guid",
    ),
    // ADR 0071 §3 names the GUID of a named feed as the publisher view does.
    ("NamedFeedEntry", "remote_feed_title", "publisher_feed_guid"),
];

/// Each route that reads `include`: (path, the function of `src/query.rs`
/// that reads it, the names that the function acts on). ADR 0071 §1c.
const INCLUDE_ROUTES: &[(&str, &str, &[&str])] = &[
    (
        "/v1/feeds/{guid}",
        "build_feed_response",
        stophammer::query::FEED_INCLUDES,
    ),
    (
        "/v1/tracks/{guid}",
        "build_track_response",
        stophammer::query::TRACK_INCLUDES,
    ),
    (
        "/v1/feeds/{guid}/tracks/{track_guid}",
        "build_track_response",
        stophammer::query::TRACK_INCLUDES,
    ),
    (
        "/v1/feeds/recent",
        "handle_get_recent_feeds",
        stophammer::query::FEED_LIST_INCLUDES,
    ),
    (
        "/v1/feeds/by-name",
        "handle_get_feeds_by_name",
        stophammer::query::FEED_LIST_INCLUDES,
    ),
    (
        "/v1/artist-names",
        "handle_get_artist_names",
        stophammer::query::ARTIST_NAME_INCLUDES,
    ),
];

fn primary_document() -> Value {
    serde_json::to_value(stophammer::openapi::primary_document()).expect("document")
}

fn readonly_document() -> Value {
    serde_json::to_value(stophammer::openapi::readonly_document()).expect("document")
}

// ── Rule 1a ──────────────────────────────────────────────────────────────────

/// Each object of `value` with a `<prefix>feed_title` property and no GUID
/// property for it, as `"<location>.<property>"`.
fn feed_title_violations(location: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(properties)) = map.get("properties") {
                let schema = location.rsplit('/').next().unwrap_or(location);
                for title in properties.keys().filter(|k| k.ends_with("feed_title")) {
                    let guid = TITLE_GUID_PAIRS
                        .iter()
                        .find(|(s, t, _)| *s == schema && t == title)
                        .map_or_else(
                            || format!("{}feed_guid", title.trim_end_matches("feed_title")),
                            |(_, _, guid)| (*guid).to_string(),
                        );
                    if !properties.contains_key(&guid) {
                        out.push(format!("{location}.{title} (expected {guid})"));
                    }
                }
            }
            for (key, child) in map {
                feed_title_violations(&format!("{location}/{key}"), child, out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                feed_title_violations(&format!("{location}/{index}"), child, out);
            }
        }
        _ => {}
    }
}

fn assert_each_feed_title_has_its_guid(document: &Value, name: &str) {
    let mut violations = Vec::new();
    feed_title_violations(
        "#/components/schemas",
        &document["components"]["schemas"],
        &mut violations,
    );
    feed_title_violations("#/paths", &document["paths"], &mut violations);
    assert!(
        violations.is_empty(),
        "ADR 0071 §1a: in the {name} document, a field gives the title of a feed with no GUID \
         field for that feed:\n  {}\nAdd the field `<prefix>feed_guid` to the same type. When \
         the GUID field has another name, add the pair to TITLE_GUID_PAIRS in this test.",
        violations.join("\n  ")
    );
}

#[test]
fn each_feed_title_has_its_guid_in_the_primary_document() {
    assert_each_feed_title_has_its_guid(&primary_document(), "primary");
}

#[test]
fn each_feed_title_has_its_guid_in_the_readonly_document() {
    assert_each_feed_title_has_its_guid(&readonly_document(), "readonly");
}

#[test]
fn the_feed_title_guard_finds_a_title_with_no_guid() {
    let schemas = json!({
        "Row": { "type": "object", "properties": {
            "publisher_feed_title": { "type": "string" },
            "feed_guid": { "type": "string" }
        }},
        "Paired": { "type": "object", "properties": {
            "remote_feed_title": { "type": "string" },
            "remote_feed_guid": { "type": "string" }
        }}
    });
    let mut violations = Vec::new();
    feed_title_violations("#/components/schemas", &schemas, &mut violations);
    assert_eq!(
        violations,
        vec!["#/components/schemas/Row.publisher_feed_title (expected publisher_feed_guid)"],
        "the guard must find only the title with no GUID of its own prefix"
    );
}

#[test]
fn each_title_guid_pair_names_existing_properties() {
    let document = primary_document();
    for (schema, title, guid) in TITLE_GUID_PAIRS {
        let properties = &document["components"]["schemas"][schema]["properties"];
        assert!(
            properties.get(title).is_some() && properties.get(guid).is_some(),
            "ADR 0071 §1a: TITLE_GUID_PAIRS names {schema}.{title} and {schema}.{guid}, but the \
             document does not hold both. Remove the stale pair from this test."
        );
    }
}

// ── Rule 1c ──────────────────────────────────────────────────────────────────

/// The `x-include-names` of the `include` parameter of each path of
/// `document` that declares one.
fn declared_includes(document: &Value) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for (path, item) in document["paths"].as_object().expect("paths") {
        for operation in item.as_object().expect("path item").values() {
            let Some(parameters) = operation.get("parameters").and_then(Value::as_array) else {
                continue;
            };
            for parameter in parameters.iter().filter(|p| p["name"] == "include") {
                let names = parameter["x-include-names"]
                    .as_array()
                    .unwrap_or_else(|| {
                        panic!(
                            "ADR 0071 §1c: the `include` parameter of {path} declares no \
                             `x-include-names`. Build it with include_query_param() in \
                             src/openapi.rs."
                        )
                    })
                    .iter()
                    .map(|name| name.as_str().expect("an include name").to_string())
                    .collect();
                out.push((path.clone(), names));
            }
        }
    }
    out
}

/// Each function of `src` that calls `.includes(`, with the string
/// literals of those calls.
fn functions_that_read_include(src: &str) -> Vec<(String, BTreeSet<String>)> {
    let mut out: Vec<(String, BTreeSet<String>)> = Vec::new();
    let mut current: Option<String> = None;
    for line in src.lines() {
        let trimmed = line.trim_start();
        let signature = trimmed
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub ")
            .trim_start_matches("async ");
        if let Some(rest) = signature.strip_prefix("fn ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            current = Some(name);
        }
        let mut rest = line;
        while let Some(pos) = rest.find(".includes(") {
            let function = current
                .clone()
                .expect("an `.includes(` call outside a function");
            let args = &rest[pos + ".includes(".len()..];
            let literal = args
                .strip_prefix('"')
                .and_then(|a| a.split('"').next())
                .map(str::to_string);
            let entry = if let Some(i) = out.iter().position(|(f, _)| *f == function) {
                &mut out[i]
            } else {
                out.push((function, BTreeSet::new()));
                out.last_mut().expect("the pushed entry")
            };
            entry.1.extend(literal);
            rest = args;
        }
    }
    out
}

fn query_source() -> String {
    let path = format!("{}/src/query.rs", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn assert_includes_declared(document: &Value, name: &str) {
    let declared = declared_includes(document);
    for (path, names) in &declared {
        let Some((_, _, expected)) = INCLUDE_ROUTES.iter().find(|(p, _, _)| p == path) else {
            panic!(
                "ADR 0071 §1c: the {name} document declares `include` on {path}, but no code \
                 reads it. Remove the parameter, or add the route to INCLUDE_ROUTES in this test."
            );
        };
        assert_eq!(
            names, expected,
            "ADR 0071 §1c: the {name} document declares other include names on {path} than \
             the code acts on. Build the parameter from the constant that the route reads."
        );
    }
    for (path, _, _) in INCLUDE_ROUTES {
        if document["paths"].get(path).is_none() {
            continue;
        }
        assert!(
            declared.iter().any(|(p, _)| p == path),
            "ADR 0071 §1c: {path} acts on `include`, but the {name} document does not declare \
             it. Add include_query_param() with the constant of the route in src/openapi.rs."
        );
    }
}

#[test]
fn each_include_is_declared_in_the_primary_document() {
    assert_includes_declared(&primary_document(), "primary");
}

#[test]
fn each_include_is_declared_in_the_readonly_document() {
    assert_includes_declared(&readonly_document(), "readonly");
}

#[test]
fn each_function_that_reads_include_has_a_declared_route() {
    let functions = functions_that_read_include(&query_source());
    assert!(
        functions.len() >= 4,
        "the test found {} functions that read `include`, fewer than the source holds",
        functions.len()
    );
    for (function, literals) in &functions {
        let routes: Vec<_> = INCLUDE_ROUTES
            .iter()
            .filter(|(_, f, _)| f == function)
            .collect();
        assert!(
            !routes.is_empty(),
            "ADR 0071 §1c: `{function}` in src/query.rs reads `include`, but INCLUDE_ROUTES \
             names no route for it. Add the route and the constant that it reads to this test, \
             and declare the parameter with include_query_param() in src/openapi.rs."
        );
        for (path, _, names) in routes {
            for literal in literals {
                assert!(
                    names.contains(&literal.as_str()),
                    "ADR 0071 §1c: `{function}` acts on the include name `{literal}`, but the \
                     constant of {path} does not list it. Add the name to that constant."
                );
            }
        }
    }
}

// ── Section 3: publisher_feed_guid ───────────────────────────────────────────

const TOKEN: &str = "adr0071-row-link-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0071-row-link-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0071-row-link-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn send(
    st: &Arc<stophammer::api::AppState>,
    method: &str,
    uri: &str,
    body: Option<&Value>,
) -> Value {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            req = req.header("Content-Type", "application/json");
            Body::from(serde_json::to_vec(value).expect("serialize"))
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
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(
        status.is_success(),
        "{method} {uri} failed with {status}: {json}"
    );
    json
}

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    remote_items: Value,
) {
    let url = format!("https://example.com/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": format!("Feed {guid}"),
            "raw_medium": medium,
            "explicit": false,
            "author_name": "Artist",
            "remote_items": remote_items,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

#[tokio::test]
async fn a_release_row_gives_the_guid_of_its_publisher_feed() {
    let st = state(common::test_db_arc());
    ingest(&st, "pub-1", "publisher", json!([])).await;
    ingest(
        &st,
        "album-1",
        "music",
        json!([{
            "position": 0,
            "medium": "publisher",
            "remote_feed_guid": "pub-1",
            "remote_feed_url": "https://example.com/pub-1.xml",
            "publisher_reference": true,
        }]),
    )
    .await;
    ingest(&st, "album-2", "music", json!([])).await;

    for uri in ["/v1/feeds/recent", "/v1/feeds/by-name", "/v1/feeds/album-1"] {
        let body = send(&st, "GET", uri, None).await;
        let rows = match &body["data"] {
            Value::Array(rows) => rows.clone(),
            row => vec![row.clone()],
        };
        let named = rows
            .iter()
            .find(|r| r["feed_guid"] == "album-1")
            .unwrap_or_else(|| panic!("{uri}: no album row in {body}"));
        assert_eq!(
            named["publisher_feed_title"], "Feed pub-1",
            "{uri}: the album row gives the title of its publisher feed"
        );
        assert_eq!(
            named["publisher_feed_guid"], "pub-1",
            "{uri}: ADR 0071 §3 gives the GUID of the same feed beside its title"
        );
        if let Some(unnamed) = rows.iter().find(|r| r["feed_guid"] == "album-2") {
            assert!(
                unnamed["publisher_feed_title"].is_null()
                    && unnamed["publisher_feed_guid"].is_null(),
                "{uri}: a row with a null title gives a null GUID: {unnamed}"
            );
        } else {
            assert!(
                uri.ends_with("album-1"),
                "{uri}: no row for album-2 in {body}"
            );
        }
    }
}
