// ADR 0044 task 003: each route of the router is in the document.
//
// docs/tasks/adr-0044-task-003-contract-guards.md
//
// axum gives no list of the routes of a `Router`, so this test reads the
// router functions from the source. It takes the whole body of each function
// by brace matching, and the path and the methods of each `.route(...)` call
// by parenthesis matching, so a call on more than one line counts. A
// self-check makes sure that the test reads each `.route(` call of each body.
//
// The document is read in-process. A test must not run `cargo` itself: a
// nested build waits on the build lock of the outer `cargo test`.

use std::collections::BTreeSet;

use serde_json::Value;

/// Routes in a router that the document does not describe, with the reason.
const UNDOCUMENTED_ROUTES: &[(&str, &str, &str)] = &[
    ("get", "/health", "liveness probe that answers plain text"),
    ("get", "/api", "static HTML explorer page"),
    ("get", "/api.html", "static HTML explorer page"),
    ("get", "/openapi.json", "the document itself"),
];

const METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

type Route = (String, String);

/// The source of the function `signature` in `file`, from the signature to
/// its matching closing brace.
fn function_body(file: &str, signature: &str) -> String {
    let path = format!("{}/{file}", env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("{file} has no `{signature}`"));
    let open = start + src[start..].find('{').expect("function body");
    let mut depth = 0;
    for (offset, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[start..=open + offset].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{file}: `{signature}` has no closing brace")
}

/// Each (method, path) of each `.route(...)` call in `body`.
fn routes_in(body: &str) -> Vec<Route> {
    let mut routes = Vec::new();
    let mut calls = 0;
    let mut rest = body;
    while let Some(pos) = rest.find(".route(") {
        let args_start = pos + ".route(".len();
        let mut depth = 1;
        let mut end = args_start;
        for (offset, c) in rest[args_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = args_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let args = &rest[args_start..end];
        let quote_start = args.find('"').expect("a route path literal") + 1;
        let quote_end = quote_start + args[quote_start..].find('"').expect("closing quote");
        let path = &args[quote_start..quote_end];
        let handlers = &args[quote_end..];
        let mut methods = 0;
        for method in METHODS {
            let call = format!("{method}(");
            let starts_call = |i: usize| {
                i == 0
                    || !handlers.as_bytes()[i - 1].is_ascii_alphanumeric()
                        && handlers.as_bytes()[i - 1] != b'_'
            };
            if handlers.match_indices(&call).any(|(i, _)| starts_call(i)) {
                routes.push(((*method).to_string(), path.to_string()));
                methods += 1;
            }
        }
        assert!(
            methods > 0,
            "the route {path} has no method this test knows"
        );
        calls += 1;
        rest = &rest[end..];
    }
    assert_eq!(
        calls,
        body.matches(".route(").count(),
        "the test must read each .route( call of the router function"
    );
    routes
}

fn documented(doc: &Value) -> BTreeSet<Route> {
    let mut out = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().expect("paths") {
        for (method, op) in item.as_object().expect("path item") {
            if op.get("responses").is_some() {
                out.insert((method.clone(), path.clone()));
            }
        }
    }
    out
}

fn assert_routes_documented(router_fn: &str, routes: &[Route], document: &Value) {
    let in_document = documented(document);
    for (method, path) in routes {
        if UNDOCUMENTED_ROUTES
            .iter()
            .any(|(m, p, _)| m == method && p == path)
        {
            continue;
        }
        assert!(
            in_document.contains(&(method.clone(), path.clone())),
            "ADR 0044: {} {path} is in {router_fn} but not in the OpenAPI document.\n\
             Add the path to spec_value() in src/openapi.rs, with the $ref of its type.",
            method.to_uppercase()
        );
    }
}

#[test]
fn each_router_route_is_in_primary_document() {
    let mut routes = routes_in(&function_body("src/api.rs", "pub fn build_router("));
    let query = routes_in(&function_body("src/query.rs", "pub fn query_routes("));
    assert!(
        routes.len() >= 17 && query.len() >= 15,
        "the test read {} router routes and {} query routes, fewer than the source holds",
        routes.len(),
        query.len()
    );
    routes.extend(query);
    let doc = serde_json::to_value(stophammer::openapi::primary_document()).expect("document");
    assert_routes_documented("build_router", &routes, &doc);
}

#[test]
fn each_readonly_router_route_is_in_readonly_document() {
    let body = function_body("src/api.rs", "pub fn build_readonly_router(");
    let mut routes = routes_in(&body);
    if body.contains("query::query_routes()") {
        routes.extend(routes_in(&function_body(
            "src/query.rs",
            "pub fn query_routes(",
        )));
    }
    assert!(!routes.is_empty(), "the readonly router has no route");
    let doc = serde_json::to_value(stophammer::openapi::readonly_document()).expect("document");
    assert_routes_documented("build_readonly_router", &routes, &doc);
}
