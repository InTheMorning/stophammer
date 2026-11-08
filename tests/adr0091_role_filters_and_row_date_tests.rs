//! ADR 0091: the role filters on `GET /v1/artist-names`, and the date of a
//! row of `include=publisher` on `GET /v1/feeds/{guid}`.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0091-role-filter-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0091-role-filter-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0091-role-filter-admin-token".into(),
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
    date: Option<i64>,
) {
    let tracks: Vec<Value> = date
        .map(|d| {
            vec![json!({
                "track_guid": format!("{guid}-t0"),
                "title": "Track 0",
                "pub_date": d,
                "explicit": false,
            })]
        })
        .unwrap_or_default();
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

/// Stores a publisher with real links. `links` holds `(rel, two_way, date)` for
/// each album. A two-way album names the publisher back, and a one-way album
/// does not. A role on a one-way link is not a stated role (ADR 0068 §1).
async fn store_publisher(
    st: &Arc<stophammer::api::AppState>,
    guid: &str,
    title: &str,
    links: &[(&str, bool, i64)],
) {
    let back = json!([{
        "position": 0,
        "medium": "publisher",
        "remote_feed_guid": guid,
        "remote_feed_url": format!("https://p.example/{guid}.xml"),
        "publisher_reference": true,
    }]);
    let mut entries = Vec::new();
    for (n, (rel, two_way, date)) in links.iter().enumerate() {
        let album = format!("alb-{guid}-{n}");
        let remote = if *two_way { back.clone() } else { json!([]) };
        ingest(
            st,
            &album,
            &format!("Album {guid} {n}"),
            "music",
            "Artist",
            remote,
            Some(*date),
        )
        .await;
        entries.push(json!({
            "position": n,
            "medium": "music",
            "remote_feed_guid": album,
            "remote_feed_url": format!("https://p.example/{album}.xml"),
            "rel": rel,
        }));
    }
    ingest(
        st,
        guid,
        title,
        "publisher",
        "Artist",
        Value::Array(entries),
        None,
    )
    .await;
}

/// Eight publishers in name order, and two `name` rows.
///
/// - Alpha: label (two-way, 1000). Bravo: artist (two-way, 5000).
/// - Charlie: label (one-way). Delta: none.
/// - Echo: label 3000 and artist 4000 (two-way). Foxtrot: artist (one-way).
/// - Golf: label (two-way, 2000). Hotel: none.
async fn stored() -> Arc<stophammer::api::AppState> {
    let st = state(common::test_db_arc());
    store_publisher(&st, "p-alpha", "Alpha", &[("label", true, 1000)]).await;
    store_publisher(&st, "p-bravo", "Bravo", &[("artist", true, 5000)]).await;
    store_publisher(&st, "p-charlie", "Charlie", &[("label", false, 9000)]).await;
    store_publisher(&st, "p-delta", "Delta", &[]).await;
    store_publisher(
        &st,
        "p-echo",
        "Echo",
        &[("label", true, 3000), ("artist", true, 4000)],
    )
    .await;
    store_publisher(&st, "p-foxtrot", "Foxtrot", &[("artist", false, 8000)]).await;
    store_publisher(&st, "p-golf", "Golf", &[("label", true, 2000)]).await;
    store_publisher(&st, "p-hotel", "Hotel", &[]).await;
    ingest(
        &st,
        "n1",
        "Solo One",
        "music",
        "Bee Name",
        json!([]),
        Some(6000),
    )
    .await;
    ingest(
        &st,
        "n2",
        "Solo Two",
        "music",
        "Zed Name",
        json!([]),
        Some(7000),
    )
    .await;
    st
}

/// The `(kind, key)` of each row of `uri` over every page of size `limit`.
async fn walk(
    st: &Arc<stophammer::api::AppState>,
    uri: &str,
    limit: usize,
) -> (Vec<(String, String)>, Vec<bool>) {
    let mut rows = Vec::new();
    let mut flags = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..200 {
        let mut full = format!("{uri}&limit={limit}");
        if let Some(c) = &cursor {
            full = format!("{full}&cursor={c}");
        }
        let body = ok(st, &full).await;
        for row in body["data"].as_array().expect("data array") {
            rows.push((
                row["kind"].as_str().expect("kind").to_string(),
                row["key"].as_str().expect("key").to_string(),
            ));
        }
        let more = body["pagination"]["has_more"] == true;
        flags.push(more);
        if !more {
            return (rows, flags);
        }
        cursor = Some(
            body["pagination"]["cursor"]
                .as_str()
                .expect("a page with has_more true gives a cursor")
                .to_string(),
        );
    }
    panic!("the walk of {uri} did not end");
}

async fn keys(st: &Arc<stophammer::api::AppState>, uri: &str, limit: usize) -> Vec<String> {
    walk(st, uri, limit)
        .await
        .0
        .into_iter()
        .map(|(_, k)| k)
        .collect()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

fn rev(items: &[&str]) -> Vec<String> {
    items.iter().rev().map(|s| (*s).to_string()).collect()
}

fn set(items: Vec<String>) -> BTreeSet<String> {
    items.into_iter().collect()
}

async fn recent_guids(st: &Arc<stophammer::api::AppState>, query: &str) -> BTreeSet<String> {
    let body = ok(
        st,
        &format!("/v1/feeds/recent?medium=publisher&limit=50{query}"),
    )
    .await;
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|r| r["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect()
}

async fn name_publisher_guids(
    st: &Arc<stophammer::api::AppState>,
    query: &str,
) -> BTreeSet<String> {
    let body = ok(st, &format!("/v1/artist-names?limit=50{query}")).await;
    body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .filter(|r| r["kind"] == "publisher")
        .map(|r| r["feed_guid"].as_str().expect("feed_guid").to_string())
        .collect()
}

#[tokio::test]
async fn the_filtered_list_has_the_publisher_set_of_feeds_recent() {
    let st = stored().await;
    for query in [
        "",
        "&stated_rel=artist",
        "&stated_rel=label",
        "&two_way_links=none",
    ] {
        let names = name_publisher_guids(&st, query).await;
        let recent = recent_guids(&st, query).await;
        assert_eq!(
            names, recent,
            "ADR 0091 §1: artist-names and feeds/recent?medium=publisher agree for '{query}'"
        );
    }
    assert_eq!(
        name_publisher_guids(&st, "&stated_rel=label").await,
        set(strs(&["p-alpha", "p-echo", "p-golf"])),
        "ADR 0091 §1: the label set is the three two-way label publishers"
    );
    assert_eq!(
        name_publisher_guids(&st, "&stated_rel=artist").await,
        set(strs(&["p-bravo", "p-echo"])),
        "ADR 0091 §1: the artist set is the two two-way artist publishers"
    );
    assert_eq!(
        name_publisher_guids(&st, "&two_way_links=none").await,
        set(strs(&["p-charlie", "p-delta", "p-foxtrot", "p-hotel"])),
        "ADR 0091 §1: two_way_links=none keeps the publishers with no two-way link"
    );
}

#[tokio::test]
async fn a_filter_gives_no_name_row() {
    let st = stored().await;
    let unfiltered = ok(&st, "/v1/artist-names?limit=50").await;
    assert!(
        unfiltered["data"]
            .as_array()
            .expect("data array")
            .iter()
            .any(|r| r["kind"] == "name"),
        "ADR 0091 §1: the fixture has name rows when no filter is set"
    );
    for query in [
        "stated_rel=artist",
        "stated_rel=label",
        "two_way_links=none",
        "stated_rel=label&sort=date",
    ] {
        let (rows, _) = walk(&st, &format!("/v1/artist-names?{query}"), 50).await;
        assert!(
            !rows.is_empty() && rows.iter().all(|(kind, _)| kind == "publisher"),
            "ADR 0091 §1: '{query}' gives no name row: {rows:?}"
        );
    }
}

#[tokio::test]
async fn sort_date_with_a_filter_orders_by_the_newest_release() {
    let st = stored().await;
    let cases: [(&str, &[&str]); 2] = [
        ("stated_rel=label", &["p-echo", "p-golf", "p-alpha"]),
        ("stated_rel=artist", &["p-bravo", "p-echo"]),
    ];
    for (query, expected) in cases {
        let base = format!("/v1/artist-names?sort=date&{query}");
        assert_eq!(
            keys(&st, &base, 50).await,
            strs(expected),
            "ADR 0091 §1: sort=date with {query} gives the newest release first"
        );
        assert_eq!(
            keys(&st, &format!("{base}&reverse=true"), 50).await,
            rev(expected),
            "ADR 0091 §1: sort=date&reverse=true with {query} gives the oldest first"
        );
    }
}

#[tokio::test]
async fn pages_of_one_give_each_matching_row_once_in_each_direction() {
    let st = stored().await;
    let cases: [(&str, &[&str]); 4] = [
        (
            "sort=name&stated_rel=label",
            &["p-alpha", "p-echo", "p-golf"],
        ),
        (
            "sort=date&stated_rel=label",
            &["p-echo", "p-golf", "p-alpha"],
        ),
        (
            "sort=name&two_way_links=none",
            &["p-charlie", "p-delta", "p-foxtrot", "p-hotel"],
        ),
        ("sort=name&stated_rel=artist", &["p-bravo", "p-echo"]),
    ];
    for (query, expected) in cases {
        for (suffix, want) in [("", strs(expected)), ("&reverse=true", rev(expected))] {
            let uri = format!("/v1/artist-names?{query}{suffix}");
            let (rows, flags) = walk(&st, &uri, 1).await;
            let got: Vec<String> = rows.into_iter().map(|(_, k)| k).collect();
            assert_eq!(
                got, want,
                "ADR 0091 §1: limit=1 paging gives each row once for {uri}"
            );
            assert_eq!(
                flags.len(),
                want.len(),
                "ADR 0091 §1: one page for each row for {uri}"
            );
            assert_eq!(
                flags.last(),
                Some(&false),
                "ADR 0091 §1: the last page has has_more false for {uri}"
            );
            assert!(
                flags[..flags.len() - 1].iter().all(|f| *f),
                "ADR 0091 §1: each earlier page has has_more true for {uri}"
            );
        }
    }
}

#[tokio::test]
async fn from_with_a_filter_starts_at_the_first_matching_row() {
    let st = stored().await;
    for (from, expected) in [
        ("a", &["p-alpha", "p-echo", "p-golf"][..]),
        ("b", &["p-echo", "p-golf"][..]),
        ("echo", &["p-echo", "p-golf"][..]),
        ("f", &["p-golf"][..]),
        ("h", &[][..]),
    ] {
        let uri = format!("/v1/artist-names?stated_rel=label&from={from}");
        assert_eq!(
            keys(&st, &uri, 50).await,
            strs(expected),
            "ADR 0091 §1: from={from} starts at the first label row at or after the text"
        );
        assert_eq!(
            keys(&st, &uri, 1).await,
            strs(expected),
            "ADR 0091 §1: from={from} with limit=1 gives the same rows"
        );
    }
}

#[tokio::test]
async fn bad_filter_values_give_400() {
    let st = stored().await;
    for uri in [
        "/v1/artist-names?stated_rel=",
        "/v1/artist-names?stated_rel=artist,label",
        "/v1/artist-names?two_way_links=some",
    ] {
        let (status, body) = get(&st, uri).await;
        assert_eq!(status, 400, "ADR 0091 §1: {uri} gives 400, got {body}");
    }
}

#[tokio::test]
async fn a_publisher_row_gives_the_date_of_the_named_feed() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        "album-dated",
        "Dated",
        "music",
        "A",
        json!([]),
        Some(7000),
    )
    .await;
    ingest(&st, "album-plain", "Plain", "music", "B", json!([]), None).await;
    let entry = |position: i64, medium: &str, guid: &str| {
        json!({
            "position": position,
            "medium": medium,
            "remote_feed_guid": guid,
            "remote_feed_url": format!("https://p.example/{guid}.xml"),
        })
    };
    ingest(
        &st,
        "pub-p",
        "Pub P",
        "publisher",
        "A",
        json!([
            entry(0, "music", "album-dated"),
            entry(1, "music", "album-plain"),
            entry(2, "music", "album-missing"),
        ]),
        None,
    )
    .await;
    let body = ok(&st, "/v1/feeds/pub-p?include=publisher").await;
    let rows = body["data"]["publisher"]
        .as_array()
        .expect("publisher rows");
    let find = |guid: &str| -> &Value {
        rows.iter()
            .find(|r| r["remote_feed_guid"] == guid)
            .unwrap_or_else(|| panic!("no row for {guid} in {body}"))
    };
    for row in rows {
        assert!(
            row.get("newest_item_at").is_some(),
            "ADR 0091 §2: the key newest_item_at is always present: {row}"
        );
    }
    let dated = find("album-dated");
    assert_eq!(
        dated["direction"], "publisher_to_music",
        "ADR 0091 §2: the fixture row is a publisher_to_music row"
    );
    let album = ok(&st, "/v1/feeds/album-dated").await;
    assert_eq!(
        dated["newest_item_at"], album["data"]["newest_item_at"],
        "ADR 0091 §2: the row gives the newest_item_at of the album"
    );
    assert_eq!(dated["newest_item_at"], json!(7000), "ADR 0091 §2");
    assert_eq!(
        find("album-plain")["newest_item_at"],
        Value::Null,
        "ADR 0091 §2: a held feed with no dated item gives null"
    );
    assert_eq!(
        find("album-missing")["newest_item_at"],
        Value::Null,
        "ADR 0091 §2: an entry the node does not hold gives null"
    );
}

#[tokio::test]
async fn capabilities_list_both_flags() {
    let st = stored().await;
    let body = ok(&st, "/v1/node/capabilities").await;
    let caps = body["capabilities"].as_array().expect("capabilities");
    for flag in ["artist_role_filter", "publisher_row_date"] {
        assert!(
            caps.iter().any(|c| c == flag),
            "ADR 0091 §3: capabilities list {flag}: {body}"
        );
    }
}

/// The fixture with `p-delta` set to no release artist, and `p-hotel` set to
/// an owner-sourced one. The ingest path always sets both columns, so the
/// test writes the rows directly.
async fn stored_with_release_artists() -> Arc<stophammer::api::AppState> {
    let st = stored().await;
    {
        let conn = st.db.writer().lock().expect("lock writer");
        conn.execute(
            "UPDATE feeds SET release_artist = NULL, release_artist_source = NULL \
             WHERE feed_guid = 'p-delta'",
            [],
        )
        .expect("clear the release artist of p-delta");
        conn.execute(
            "UPDATE feeds SET release_artist = 'Hotel Owner', \
             release_artist_source = 'itunes_owner' WHERE feed_guid = 'p-hotel'",
            [],
        )
        .expect("set the release artist of p-hotel");
    }
    st
}

/// Every row of `uri` over all pages of size 3.
async fn rows_of(st: &Arc<stophammer::api::AppState>, uri: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..200 {
        let mut full = format!("{uri}&limit=3");
        if let Some(c) = &cursor {
            full = format!("{full}&cursor={c}");
        }
        let body = ok(st, &full).await;
        rows.extend(body["data"].as_array().expect("data array").clone());
        if body["pagination"]["has_more"] != true {
            return rows;
        }
        cursor = Some(
            body["pagination"]["cursor"]
                .as_str()
                .expect("cursor")
                .to_string(),
        );
    }
    panic!("the walk of {uri} did not end");
}

#[tokio::test]
async fn a_publisher_row_gives_the_release_artist_of_feeds_recent() {
    let st = stored_with_release_artists().await;
    let recent: HashMap<String, Value> = rows_of(&st, "/v1/feeds/recent?medium=publisher&x=1")
        .await
        .into_iter()
        .map(|r| (r["feed_guid"].as_str().expect("feed_guid").to_string(), r))
        .collect();
    assert_eq!(recent.len(), 8, "ADR 0091 §1a: eight publisher feeds");
    let names = rows_of(&st, "/v1/artist-names?include=link_facts").await;
    let mut seen = 0;
    for row in names.iter().filter(|r| r["kind"] == "publisher") {
        let guid = row["feed_guid"].as_str().expect("feed_guid");
        let want = &recent[guid];
        for key in ["release_artist", "release_artist_source"] {
            assert!(
                row.get(key).is_some(),
                "ADR 0091 §1a: the key {key} is present on the row of {guid}: {row}"
            );
            assert_eq!(
                row[key], want[key],
                "ADR 0091 §1a: {key} of {guid} equals the value of feeds/recent"
            );
        }
        seen += 1;
    }
    assert_eq!(seen, 8, "ADR 0091 §1a: eight publisher rows");

    let by_guid = |guid: &str| -> &Value {
        names
            .iter()
            .find(|r| r["kind"] == "publisher" && r["feed_guid"] == guid)
            .unwrap_or_else(|| panic!("no row for {guid}"))
    };
    let alpha = by_guid("p-alpha");
    assert_eq!(
        alpha["release_artist"], "Artist",
        "ADR 0091 §1a: a feed with an author gives its release_artist"
    );
    assert_eq!(
        alpha["release_artist_source"], "itunes_author",
        "ADR 0091 §1a: the source of the author is itunes_author"
    );
    let hotel = by_guid("p-hotel");
    assert_eq!(hotel["release_artist"], "Hotel Owner", "ADR 0091 §1a");
    assert_eq!(
        hotel["release_artist_source"], "itunes_owner",
        "ADR 0091 §1a"
    );
    let delta = by_guid("p-delta");
    for key in ["release_artist", "release_artist_source"] {
        assert_eq!(
            delta.get(key),
            Some(&Value::Null),
            "ADR 0091 §1a: {key} is a present key with a null value when the feed has none: {delta}"
        );
    }
}

#[tokio::test]
async fn the_release_artist_keys_need_link_facts_and_a_publisher_row() {
    let st = stored_with_release_artists().await;
    for uri in [
        "/v1/artist-names?x=1",
        "/v1/artist-names?include=link_facts",
    ] {
        let rows = rows_of(&st, uri).await;
        assert!(
            rows.iter().any(|r| r["kind"] == "name"),
            "ADR 0091 §1a: {uri} lists a name row"
        );
        for row in &rows {
            if row["kind"] == "name" {
                for key in ["release_artist", "release_artist_source"] {
                    assert!(
                        row.get(key).is_none(),
                        "ADR 0091 §1a: a name row never has {key} ({uri}): {row}"
                    );
                }
            } else if !uri.contains("link_facts") {
                for key in ["release_artist", "release_artist_source"] {
                    assert!(
                        row.get(key).is_none(),
                        "ADR 0091 §1a: without include=link_facts no row has {key}: {row}"
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn capabilities_list_artist_row_release_artist() {
    let st = stored().await;
    let body = ok(&st, "/v1/node/capabilities").await;
    let caps = body["capabilities"].as_array().expect("capabilities");
    assert!(
        caps.iter().any(|c| c == "artist_row_release_artist"),
        "ADR 0091 §1a: capabilities list artist_row_release_artist: {body}"
    );
}
