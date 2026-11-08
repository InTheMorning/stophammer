//! ADR 0089: the newest release of an artist row, `sort=date` on
//! `GET /v1/artist-names`, and `artist_key` on `GET /v1/feeds/recent`.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0089-artist-key-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0089-artist-key-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0089-artist-key-admin-token".into(),
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

async fn ingest_as(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    medium: &str,
    author: &str,
    items: Value,
    dates: &[Option<i64>],
) {
    let tracks: Vec<Value> = dates
        .iter()
        .enumerate()
        .map(|(i, date)| {
            json!({
                "track_guid": format!("{guid}-t{i}"),
                "title": format!("Track {i}"),
                "pub_date": date,
                "explicit": false,
            })
        })
        .collect();
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
            "author_name": author,
            "remote_items": items,
            "tracks": tracks,
        }
    });
    send(st, "POST", "/ingest/feed", Some(&payload)).await;
}

/// An item of an album inside `<podcast:publisher>` that names `publisher`.
fn names(position: i64, publisher: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "publisher",
        "remote_feed_guid": publisher,
        "remote_feed_url": format!("https://example.com/{publisher}.xml"),
        "publisher_reference": true,
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// An item of a publisher feed that lists `album`.
fn lists(position: i64, album: &str, rel: Option<&str>) -> Value {
    let mut item = json!({
        "position": position,
        "medium": "music",
        "remote_feed_guid": album,
        "remote_feed_url": format!("https://example.com/{album}.xml"),
    });
    if let Some(rel) = rel {
        item["rel"] = json!(rel);
    }
    item
}

/// Stores these artists. Dates are item pubDate values.
///
/// - "Alpha Band": `a1` with 1000, `a2` with 1000 and 3000. Newest 3000.
/// - "Beta Band": `b1` with 2000. "Cora Band": `c1` with 2000. A tie.
/// - "Gamma Band": `g1` with no item date. Null.
/// - Publisher `pub-x`: the album `x1` (5000) names it with the role `artist`,
///   so the album is not at the name level. Newest 5000.
/// - Publisher `pub-empty`: no release names it. Null.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    ingest_as(&st, "a1", "music", "Alpha Band", json!([]), &[Some(1000)]).await;
    ingest_as(
        &st,
        "a2",
        "music",
        "Alpha Band",
        json!([]),
        &[Some(1000), Some(3000)],
    )
    .await;
    ingest_as(&st, "b1", "music", "Beta Band", json!([]), &[Some(2000)]).await;
    ingest_as(&st, "c1", "music", "Cora Band", json!([]), &[Some(2000)]).await;
    ingest_as(&st, "g1", "music", "Gamma Band", json!([]), &[None]).await;
    ingest_as(
        &st,
        "x1",
        "music",
        "Xeno",
        json!([names(0, "pub-x", Some("artist"))]),
        &[Some(5000)],
    )
    .await;
    ingest_as(
        &st,
        "pub-x",
        "publisher",
        "Xeno",
        json!([lists(0, "x1", Some("artist"))]),
        &[],
    )
    .await;
    ingest_as(&st, "pub-empty", "publisher", "Nobody", json!([]), &[]).await;
    st
}

async fn status_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> u16 {
    stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
        .status()
        .as_u16()
}

/// The `kind:key` of each row of `uri`, read with a page size of `limit`.
async fn row_ids(st: &Arc<stophammer::api::AppState>, uri: &str, limit: u32) -> Vec<String> {
    let mut ids = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page_uri = match &cursor {
            Some(c) => format!("{uri}&limit={limit}&cursor={c}"),
            None => format!("{uri}&limit={limit}"),
        };
        let body = send(st, "GET", &page_uri, None).await;
        for row in body["data"].as_array().expect("data is an array") {
            ids.push(format!(
                "{}:{}",
                row["kind"].as_str().expect("kind"),
                row["key"].as_str().expect("key")
            ));
        }
        match body["pagination"]["cursor"].as_str() {
            Some(c) if body["pagination"]["has_more"] == true => cursor = Some(c.to_string()),
            _ => break,
        }
    }
    ids
}

/// The feed GUIDs of `uri`, in the order given.
async fn guids_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> Vec<String> {
    let body = send(st, "GET", uri, None).await;
    body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|r| r["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[tokio::test]
async fn recent_with_artist_key_gives_the_releases_of_the_name_row_newest_first() {
    let st = stored().await;
    let recent = guids_of(&st, "/v1/feeds/recent?artist_key=alpha%20band&limit=50").await;
    assert_eq!(
        recent,
        strs(&["a2", "a1"]),
        "ADR 0089 §1: the list gives the releases of the row, newest item first"
    );
    let by_name = guids_of(&st, "/v1/feeds/by-name?artist_key=alpha%20band&limit=50").await;
    let mut a = recent.clone();
    let mut b = by_name;
    a.sort();
    b.sort();
    assert_eq!(
        a, b,
        "ADR 0089 §1: the set is the set of feeds/by-name with the same key"
    );
    let rows = send(&st, "GET", "/v1/artist-names?limit=50", None).await;
    let row = rows["data"]
        .as_array()
        .expect("data")
        .iter()
        .find(|r| r["key"] == "alpha band")
        .expect("alpha row");
    assert_eq!(
        row["release_count"],
        recent.len(),
        "ADR 0089 §1: the list agrees with release_count"
    );
}

#[tokio::test]
async fn recent_with_artist_key_and_reverse_gives_oldest_first() {
    let st = stored().await;
    let recent = guids_of(
        &st,
        "/v1/feeds/recent?artist_key=alpha%20band&reverse=true&limit=50",
    )
    .await;
    assert_eq!(
        recent,
        strs(&["a1", "a2"]),
        "ADR 0089 §1: reverse=true gives the oldest item first"
    );
}

#[tokio::test]
async fn recent_with_artist_key_rejects_another_medium_and_named_feed_guid() {
    let st = stored().await;
    assert_eq!(
        status_of(
            &st,
            "/v1/feeds/recent?artist_key=alpha%20band&medium=publisher"
        )
        .await,
        400,
        "ADR 0089 §1: artist_key needs medium=music"
    );
    assert_eq!(
        status_of(
            &st,
            "/v1/feeds/recent?artist_key=alpha%20band&named_feed_guid=pub-x"
        )
        .await,
        400,
        "ADR 0089 §1: artist_key conflicts with named_feed_guid"
    );
}

#[tokio::test]
async fn recent_with_an_unknown_key_gives_an_empty_list() {
    let st = stored().await;
    let body = send(
        &st,
        "GET",
        "/v1/feeds/recent?artist_key=nobody%20here",
        None,
    )
    .await;
    assert_eq!(
        body["data"],
        json!([]),
        "ADR 0089 §1: an unknown key gives an empty list with 200"
    );
}

#[tokio::test]
async fn each_row_has_the_newest_release_date() {
    let st = stored().await;
    let body = send(&st, "GET", "/v1/artist-names?limit=50", None).await;
    let rows = body["data"].as_array().expect("data is an array");
    let date_of = |kind: &str, key: &str| -> Value {
        let row = rows
            .iter()
            .find(|r| r["kind"] == kind && r["key"] == key)
            .unwrap_or_else(|| panic!("no row {kind}:{key} in {body}"));
        assert!(
            row.get("newest_release_at").is_some(),
            "ADR 0089 §2: row {kind}:{key} has the field newest_release_at: {row}"
        );
        row["newest_release_at"].clone()
    };
    assert_eq!(
        date_of("name", "alpha band"),
        json!(3000),
        "ADR 0089 §2: a name row gives the largest date of its releases"
    );
    assert_eq!(date_of("name", "beta band"), json!(2000), "ADR 0089 §2");
    assert_eq!(
        date_of("name", "gamma band"),
        Value::Null,
        "ADR 0089 §2: null when no release has a date"
    );
    assert_eq!(
        date_of("publisher", "pub-x"),
        json!(5000),
        "ADR 0089 §2: a publisher row gives the largest date of the releases that name it"
    );
    assert_eq!(
        date_of("publisher", "pub-empty"),
        Value::Null,
        "ADR 0089 §2: a publisher that no release names has null"
    );
}

#[tokio::test]
async fn sort_date_gives_the_newest_first_with_nulls_last_and_ties_ascending() {
    let st = stored().await;
    let ids = row_ids(&st, "/v1/artist-names?sort=date", 50).await;
    assert_eq!(
        ids,
        strs(&[
            "publisher:pub-x",
            "name:alpha band",
            "name:beta band",
            "name:cora band",
            "publisher:pub-empty",
            "name:gamma band",
        ]),
        "ADR 0089 §2: newest first, null last, ties by (sort_name, kind, key) ascending"
    );
}

#[tokio::test]
async fn sort_date_with_reverse_gives_the_oldest_first_with_nulls_last_and_ties_descending() {
    let st = stored().await;
    let ids = row_ids(&st, "/v1/artist-names?sort=date&reverse=true", 50).await;
    assert_eq!(
        ids,
        strs(&[
            "name:cora band",
            "name:beta band",
            "name:alpha band",
            "publisher:pub-x",
            "name:gamma band",
            "publisher:pub-empty",
        ]),
        "ADR 0089 §2: oldest first, null still last, ties descending"
    );
}

#[tokio::test]
async fn sort_name_and_no_sort_give_the_name_sequence() {
    let st = stored().await;
    let expected = strs(&[
        "name:alpha band",
        "name:beta band",
        "name:cora band",
        "publisher:pub-empty",
        "publisher:pub-x",
        "name:gamma band",
    ]);
    // A publisher row sorts by its feed title, "Feed pub-empty" and
    // "Feed pub-x", so both come before "gamma band".
    let none = row_ids(&st, "/v1/artist-names?x=1", 50).await;
    let name = row_ids(&st, "/v1/artist-names?sort=name", 50).await;
    assert_eq!(
        none, expected,
        "ADR 0089 §2: no sort gives the A to Z sequence"
    );
    assert_eq!(
        name, expected,
        "ADR 0089 §2: sort=name gives the A to Z sequence"
    );
}

#[tokio::test]
async fn paging_over_sort_date_gives_each_row_one_time() {
    let st = stored().await;
    for uri in [
        "/v1/artist-names?sort=date",
        "/v1/artist-names?sort=date&reverse=true",
    ] {
        let whole = row_ids(&st, uri, 50).await;
        let one = row_ids(&st, uri, 1).await;
        let two = row_ids(&st, uri, 2).await;
        assert_eq!(whole.len(), 6, "ADR 0089 §2: six rows in all for {uri}");
        assert_eq!(
            one, whole,
            "ADR 0089 §2: limit=1 paging gives the sequence of one page for {uri}"
        );
        assert_eq!(
            two, whole,
            "ADR 0089 §2: limit=2 paging gives the sequence of one page for {uri}"
        );
    }
}

#[tokio::test]
async fn bad_sort_values_and_cursors_give_400() {
    let st = stored().await;
    assert_eq!(
        status_of(&st, "/v1/artist-names?sort=date&from=a").await,
        400,
        "ADR 0089 §2: sort=date with from gives 400"
    );
    assert_eq!(
        status_of(&st, "/v1/artist-names?sort=other").await,
        400,
        "ADR 0089 §2: an unknown sort gives 400"
    );

    let date_page = send(&st, "GET", "/v1/artist-names?sort=date&limit=1", None).await;
    let date_cursor = date_page["pagination"]["cursor"]
        .as_str()
        .expect("date cursor")
        .to_string();
    assert_eq!(
        status_of(
            &st,
            &format!("/v1/artist-names?sort=name&cursor={date_cursor}")
        )
        .await,
        400,
        "ADR 0089 §2: a cursor of sort=date sent with sort=name gives 400"
    );

    let name_page = send(&st, "GET", "/v1/artist-names?sort=name&limit=1", None).await;
    let name_cursor = name_page["pagination"]["cursor"]
        .as_str()
        .expect("name cursor")
        .to_string();
    assert_eq!(
        status_of(
            &st,
            &format!("/v1/artist-names?sort=date&cursor={name_cursor}")
        )
        .await,
        400,
        "ADR 0089 §2: a cursor of sort=name sent with sort=date gives 400"
    );
}

#[tokio::test]
async fn capabilities_list_artist_newest() {
    let st = stored().await;
    let body = send(&st, "GET", "/v1/node/capabilities", None).await;
    let caps = body["capabilities"].as_array().expect("capabilities");
    assert!(
        caps.iter().any(|c| c == "artist_newest"),
        "ADR 0089 §3: capabilities list artist_newest: {body}"
    );
}
