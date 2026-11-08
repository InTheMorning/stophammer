//! ADR 0092: `release_track_counts` on the publisher rows and the name rows of
//! the artist lists.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0092-track-counts-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0092-track-counts-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0092-track-counts-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
        artist_name_cache: stophammer::query::ArtistNameCache::default(),
    })
}

async fn get(st: &Arc<stophammer::api::AppState>, uri: &str) -> (u16, Value) {
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn ok(st: &Arc<stophammer::api::AppState>, uri: &str) -> Value {
    let (status, body) = get(st, uri).await;
    assert_eq!(status, 200, "GET {uri} gave {status}: {body}");
    body
}

async fn ingest(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    medium: &str,
    author: &str,
    remote_items: Value,
    tracks: usize,
) {
    let tracks: Vec<Value> = (0..tracks)
        .map(|n| {
            json!({
                "track_guid": format!("{guid}-t{n}"),
                "title": format!("Track {n}"),
                "pub_date": 1000 + i64::try_from(n).expect("small index"),
                "explicit": false,
            })
        })
        .collect();
    let url = format!("https://p.example/{guid}.xml");
    let payload = json!({
        "canonical_url": url,
        "source_url": url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{guid}"),
        "feed_data": {
            "feed_guid": guid,
            "title": title,
            "raw_medium": medium,
            "explicit": false,
            "author_name": author,
            "remote_items": remote_items,
            "tracks": tracks,
        }
    });
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ingest/feed")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).expect("serialize")))
                .expect("build request"),
        )
        .await
        .expect("send request");
    assert!(
        resp.status().is_success(),
        "ingest of {guid} must succeed, got {}",
        resp.status()
    );
}

/// Stores a publisher with one album for each `(two_way, tracks)` entry. A
/// two-way album names the publisher back, and a one-way album does not.
async fn store_publisher(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    links: &[(bool, usize)],
) {
    let back = json!([{
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": guid,
        "remote_feed_url": format!("https://p.example/{guid}.xml"),
        "publisher_reference": true,
    }]);
    let mut entries = Vec::new();
    for (n, (two_way, tracks)) in links.iter().enumerate() {
        let album = format!("alb-{guid}-{n}");
        let remote = if *two_way { back.clone() } else { json!([]) };
        ingest(
            st,
            &album,
            &format!("Album {guid} {n}"),
            "music",
            &format!("Linked Artist {guid} {n}"),
            remote,
            *tracks,
        )
        .await;
        entries.push(json!({
            "position": n,
            "medium": "music",
            "remote_feed_guid": album,
            "remote_feed_url": format!("https://p.example/{album}.xml"),
            "rel": "label",
        }));
    }
    ingest(
        st,
        guid,
        title,
        "publisher",
        "Artist",
        Value::Array(entries),
        0,
    )
    .await;
}

/// - `p-main` (Main Label): confirmed albums of 1, 1, 1, 9 and 12 tracks.
/// - `p-oneway` (Oneway Label): a confirmed album of 2 tracks, and a one-way
///   album of 5 tracks, a count that no confirmed link has.
/// - `p-none` (None Label): no link.
/// - Name rows: Bee Name has releases of 2, 2 and 7 tracks, and Zed Name has
///   one of 3 tracks. The one-way album of `p-oneway` is a release of its own
///   name, because it holds no link to a publisher.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    store_publisher(
        &st,
        "p-main",
        "Main Label",
        &[(true, 1), (true, 1), (true, 1), (true, 9), (true, 12)],
    )
    .await;
    store_publisher(&st, "p-oneway", "Oneway Label", &[(true, 2), (false, 5)]).await;
    store_publisher(&st, "p-none", "None Label", &[]).await;
    for (guid, artist, tracks) in [
        ("n1", "Bee Name", 2),
        ("n2", "Bee Name", 2),
        ("n3", "Bee Name", 7),
        ("n4", "Zed Name", 3),
    ] {
        ingest(&st, guid, guid, "music", artist, json!([]), tracks).await;
    }
    st
}

/// The row of `guid` on each of the four routes, as `(route, row)`.
async fn publisher_rows(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    include: &str,
) -> Vec<(&'static str, Value)> {
    let routes: [(&'static str, String, &str, &str); 4] = [
        (
            "feeds/recent",
            format!("/v1/feeds/recent?medium=publisher&limit=50{include}"),
            "feed_guid",
            guid,
        ),
        (
            "feeds/by-name",
            format!("/v1/feeds/by-name?medium=publisher&limit=50{include}"),
            "feed_guid",
            guid,
        ),
        (
            "search",
            format!("/v1/search?type=publisher&q=Label&limit=50{include}"),
            "entity_id",
            guid,
        ),
        (
            "artist-names",
            format!("/v1/artist-names?limit=50{include}"),
            "feed_guid",
            guid,
        ),
    ];
    let mut out = Vec::new();
    for (route, uri, field, want) in routes {
        let body = ok(st, &uri).await;
        let row = body["data"]
            .as_array()
            .expect("data array")
            .iter()
            .find(|r| r[field] == want && r["kind"] != "name")
            .unwrap_or_else(|| panic!("no row for {want} on {route}: {body}"))
            .clone();
        out.push((route, row));
    }
    out
}

fn sum(map: &Value) -> i64 {
    map.as_object()
        .expect("a JSON object")
        .values()
        .map(|v| v.as_i64().expect("an integer count"))
        .sum()
}

#[tokio::test]
async fn confirmed_links_give_a_map_of_track_counts_on_each_route() {
    let st = stored().await;
    for (route, row) in publisher_rows(&st, "p-main", "&include=link_facts").await {
        assert_eq!(
            row["release_track_counts"],
            json!({"1": 3, "9": 1, "12": 1}),
            "ADR 0092 §1: the map of {route}: {row}"
        );
        assert_eq!(
            sum(&row["release_track_counts"]),
            row["two_way_link_count"].as_i64().expect("count"),
            "ADR 0092 §1: the values sum to two_way_link_count on {route}"
        );
    }
}

#[tokio::test]
async fn a_one_way_link_is_not_counted() {
    let st = stored().await;
    for (route, row) in publisher_rows(&st, "p-oneway", "&include=link_facts").await {
        assert_eq!(
            row["release_track_counts"],
            json!({"2": 1}),
            "ADR 0092 §1: the one-way album of 5 tracks is not counted on {route}: {row}"
        );
        assert!(
            row["release_track_counts"].get("5").is_none(),
            "ADR 0092 §1: key 5 is absent on {route}"
        );
    }
}

#[tokio::test]
async fn a_publisher_with_no_confirmed_link_gives_an_empty_map() {
    let st = stored().await;
    for (route, row) in publisher_rows(&st, "p-none", "&include=link_facts").await {
        assert_eq!(
            row["two_way_link_count"], 0,
            "ADR 0092 §1: no confirmed link on {route}"
        );
        assert_eq!(
            row["release_track_counts"],
            json!({}),
            "ADR 0092 §1: an empty map on {route}: {row}"
        );
    }
}

#[tokio::test]
async fn without_link_facts_the_key_is_absent() {
    let st = stored().await;
    for guid in ["p-main", "p-oneway", "p-none"] {
        for (route, row) in publisher_rows(&st, guid, "").await {
            assert!(
                row.get("release_track_counts").is_none(),
                "ADR 0092 §1: no release_track_counts without the include on {route} for {guid}: {row}"
            );
        }
    }
}

#[tokio::test]
async fn a_name_row_gives_a_map_that_sums_to_the_release_count() {
    let st = stored().await;
    for include in ["", "&include=link_facts"] {
        let body = ok(&st, &format!("/v1/artist-names?limit=50{include}")).await;
        let rows = body["data"].as_array().expect("data array");
        let name = |n: &str| {
            rows.iter()
                .find(|r| r["kind"] == "name" && r["name"] == n)
                .unwrap_or_else(|| panic!("no name row {n}: {body}"))
        };
        let bee = name("Bee Name");
        assert_eq!(
            bee["release_track_counts"],
            json!({"2": 2, "7": 1}),
            "ADR 0092 §1: the map of Bee Name (include {include:?}): {bee}"
        );
        let zed = name("Zed Name");
        assert_eq!(
            zed["release_track_counts"],
            json!({"3": 1}),
            "ADR 0092 §1: the map of Zed Name (include {include:?}): {zed}"
        );
        for row in rows.iter().filter(|r| r["kind"] == "name") {
            assert_eq!(
                sum(&row["release_track_counts"]),
                row["release_count"].as_i64().expect("release_count"),
                "ADR 0092 §1: the values sum to release_count (include {include:?}): {row}"
            );
        }
    }
}

#[tokio::test]
async fn capabilities_list_release_track_counts() {
    let st = stored().await;
    let body = ok(&st, "/v1/node/capabilities").await;
    let caps = body["capabilities"].as_array().expect("capabilities");
    assert!(
        caps.iter().any(|c| c == "release_track_counts"),
        "ADR 0092 §2: capabilities list release_track_counts: {body}"
    );
}
