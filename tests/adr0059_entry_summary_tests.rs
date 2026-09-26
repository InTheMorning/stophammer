// ADR 0059: an entry that names a feed gives its summary.
//
// docs/tasks/adr-0059-task-001-entry-summary-fields.md
//
// Each `publisher` and `remote_items` entry, on a feed read and on a track
// read, gives `remote_feed_title`, `remote_feed_image_url`,
// `remote_release_artist` and `remote_release_artist_source` of the feed
// that it names. Each test below is one guard of ADR 0059.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0059-crawl-token";

const SUMMARY_KEYS: [&str; 4] = [
    "remote_feed_title",
    "remote_feed_image_url",
    "remote_release_artist",
    "remote_release_artist_source",
];

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0059-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0059-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        skip_ssrf_validation: true,
    })
}

/// One feed to ingest.
struct Feed<'a> {
    guid: &'a str,
    url: &'a str,
    title: &'a str,
    medium: &'a str,
    image_url: Option<&'a str>,
    author: Option<&'a str>,
    remote_items: Value,
    tracks: Value,
}

async fn ingest(st: &Arc<stophammer::api::AppState>, feed: &Feed<'_>) {
    let payload = json!({
        "canonical_url": feed.url,
        "source_url": feed.url,
        "crawl_token": TOKEN,
        "http_status": 200,
        "content_hash": format!("hash-{}", feed.guid),
        "feed_data": {
            "feed_guid": feed.guid,
            "title": feed.title,
            "raw_medium": feed.medium,
            "explicit": false,
            "image_url": feed.image_url,
            "author_name": feed.author,
            "remote_items": feed.remote_items,
            "tracks": feed.tracks,
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
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    assert!(
        status.is_success(),
        "ingest of {} failed with {status}: {}",
        feed.guid,
        String::from_utf8_lossy(&bytes)
    );
}

async fn get(st: &Arc<stophammer::api::AppState>, uri: &str) -> Value {
    let resp = stophammer::api::build_router(Arc::clone(st))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("read body")
        .to_bytes();
    assert!(status.is_success(), "GET {uri} failed with {status}");
    serde_json::from_slice(&bytes).expect("parse json")
}

fn remote_item(position: i64, medium: &str, guid: &str, url: &str) -> Value {
    json!({
        "position": position,
        "medium": medium,
        "remote_feed_guid": guid,
        "remote_feed_url": url,
    })
}

fn album<'a>(guid: &'a str, url: &'a str, title: &'a str, author: &'a str) -> Feed<'a> {
    Feed {
        guid,
        url,
        title,
        medium: "music",
        image_url: Some("https://img.example/album.jpg"),
        author: Some(author),
        remote_items: json!([]),
        tracks: json!([]),
    }
}

fn entries<'a>(body: &'a Value, include: &str) -> &'a Vec<Value> {
    body["data"][include]
        .as_array()
        .unwrap_or_else(|| panic!("the response must hold `{include}`"))
}

fn find<'a>(items: &'a [Value], key: &str, value: &str) -> &'a Value {
    items
        .iter()
        .find(|e| e[key] == value)
        .unwrap_or_else(|| panic!("no entry with {key} = {value}"))
}

#[tokio::test]
async fn a_publisher_read_gives_the_summary_of_each_album() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        &album("album-a", "https://a.example/a.xml", "Album A", "Artist A"),
    )
    .await;
    ingest(
        &st,
        &Feed {
            guid: "publisher-p",
            url: "https://p.example/p.xml",
            title: "Publisher P",
            medium: "publisher",
            image_url: None,
            author: None,
            remote_items: json!([
                remote_item(0, "music", "album-a", "https://a.example/a.xml"),
                remote_item(1, "music", "album-missing", "https://m.example/m.xml"),
            ]),
            tracks: json!([]),
        },
    )
    .await;

    let body = get(&st, "/v1/feeds/publisher-p?include=publisher").await;
    let rows = entries(&body, "publisher");

    let row = find(rows, "music_feed_guid", "album-a");
    assert_eq!(
        row["remote_feed_title"], "Album A",
        "ADR 0059 guard 1: a publisher read gives the title of each indexed album"
    );
    assert_eq!(
        row["remote_feed_image_url"], "https://img.example/album.jpg",
        "ADR 0059 guard 1: a publisher read gives the image of each indexed album"
    );
    assert_eq!(
        row["remote_release_artist"], "Artist A",
        "ADR 0059 guard 1: a publisher read gives the artist of each indexed album"
    );
    assert_eq!(
        row["remote_release_artist_source"], "itunes_author",
        "ADR 0059 guard 1: a publisher read gives the artist source of each indexed album"
    );

    let missing = find(rows, "remote_feed_guid", "album-missing");
    for key in SUMMARY_KEYS {
        assert!(
            missing.get(key).is_some_and(Value::is_null),
            "ADR 0059 §1: an unresolved publisher entry gives the key {key} with null"
        );
    }
}

#[tokio::test]
async fn an_album_read_gives_the_summary_of_its_publisher() {
    let st = state(common::test_db_arc());
    // The publisher does not list the album back. The album still names an
    // indexed publisher, so its entry gives the publisher summary.
    ingest(
        &st,
        &Feed {
            guid: "publisher-q",
            url: "https://q.example/q.xml",
            title: "Publisher Q",
            medium: "publisher",
            image_url: Some("https://img.example/q.jpg"),
            author: Some("Label Q"),
            remote_items: json!([]),
            tracks: json!([]),
        },
    )
    .await;
    let mut b = album("album-b", "https://b.example/b.xml", "Album B", "Artist B");
    b.remote_items = json!([remote_item(
        0,
        "publisher",
        "publisher-q",
        "https://q.example/q.xml"
    )]);
    ingest(&st, &b).await;

    let body = get(&st, "/v1/feeds/album-b?include=publisher").await;
    let row = find(
        entries(&body, "publisher"),
        "publisher_feed_guid",
        "publisher-q",
    );
    assert_eq!(
        row["direction"], "music_to_publisher",
        "the album row names its publisher"
    );
    assert_eq!(
        row["remote_feed_title"], "Publisher Q",
        "ADR 0059 guard 2: an album read gives the title of its publisher, also when the publisher does not list the album"
    );
    assert_eq!(
        row["remote_feed_image_url"], "https://img.example/q.jpg",
        "ADR 0059 guard 2: an album read gives the image of its publisher"
    );
    assert_eq!(
        row["remote_release_artist"], "Label Q",
        "ADR 0059 guard 2: an album read gives the artist of its publisher"
    );
}

#[tokio::test]
async fn a_remote_items_entry_gives_the_summary_or_null() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        &album("album-c", "https://c.example/c.xml", "Album C", "Artist C"),
    )
    .await;
    ingest(
        &st,
        &Feed {
            guid: "list-l",
            url: "https://l.example/l.xml",
            title: "List L",
            medium: "musicL",
            image_url: None,
            author: None,
            remote_items: json!([
                remote_item(0, "music", "album-c", "https://c.example/c.xml"),
                remote_item(1, "music", "album-none", "https://n.example/n.xml"),
            ]),
            tracks: json!([]),
        },
    )
    .await;

    let body = get(&st, "/v1/feeds/list-l?include=remote_items").await;
    let items = entries(&body, "remote_items");

    let indexed = find(items, "remote_feed_guid", "album-c");
    assert_eq!(
        indexed["remote_feed_title"], "Album C",
        "ADR 0059 guard 3: a remote_items entry for an indexed feed gives its title"
    );
    assert_eq!(
        indexed["remote_release_artist"], "Artist C",
        "ADR 0059 guard 3: a remote_items entry for an indexed feed gives its artist"
    );

    let none = find(items, "remote_feed_guid", "album-none");
    for key in SUMMARY_KEYS {
        assert!(
            none.get(key).is_some_and(Value::is_null),
            "ADR 0059 guard 3: an entry for a feed that is not indexed gives the key {key} with null"
        );
    }
}

#[tokio::test]
async fn an_entry_that_resolves_by_url_gives_the_feed_at_that_url() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        &album("album-d", "https://d.example/d.xml", "Album D", "Artist D"),
    )
    .await;
    ingest(
        &st,
        &Feed {
            guid: "list-u",
            url: "https://u.example/u.xml",
            title: "List U",
            medium: "musicL",
            image_url: None,
            author: None,
            remote_items: json!([remote_item(
                0,
                "music",
                "guid-not-indexed",
                "https://d.example/d.xml"
            )]),
            tracks: json!([]),
        },
    )
    .await;

    let body = get(&st, "/v1/feeds/list-u?include=remote_items").await;
    let entry = find(
        entries(&body, "remote_items"),
        "remote_feed_guid",
        "guid-not-indexed",
    );
    assert_eq!(
        entry["remote_feed_title"], "Album D",
        "ADR 0059 guard 4: an entry that resolves by URL gives the summary of the feed at that URL"
    );
}

#[tokio::test]
async fn a_named_feed_with_a_javascript_image_gives_a_null_image() {
    let st = state(common::test_db_arc());
    let mut e = album("album-e", "https://e.example/e.xml", "Album E", "Artist E");
    e.image_url = Some("javascript:alert(1)");
    ingest(&st, &e).await;
    ingest(
        &st,
        &Feed {
            guid: "list-j",
            url: "https://j.example/j.xml",
            title: "List J",
            medium: "musicL",
            image_url: None,
            author: None,
            remote_items: json!([remote_item(
                0,
                "music",
                "album-e",
                "https://e.example/e.xml"
            )]),
            tracks: json!([]),
        },
    )
    .await;

    let body = get(&st, "/v1/feeds/list-j?include=remote_items").await;
    let entry = find(
        entries(&body, "remote_items"),
        "remote_feed_guid",
        "album-e",
    );
    assert_eq!(
        entry["remote_feed_title"], "Album E",
        "the entry resolves to the indexed feed"
    );
    assert!(
        entry["remote_feed_image_url"].is_null(),
        "ADR 0059 guard 5: a named feed with a javascript: image gives a null image (ADR 0054 §4)"
    );
}

#[tokio::test]
async fn a_track_read_gives_the_summary_of_each_named_feed() {
    let st = state(common::test_db_arc());
    ingest(
        &st,
        &Feed {
            guid: "publisher-t",
            url: "https://t.example/t.xml",
            title: "Publisher T",
            medium: "publisher",
            image_url: None,
            author: Some("Label T"),
            remote_items: json!([]),
            tracks: json!([]),
        },
    )
    .await;
    let mut f = album("album-f", "https://f.example/f.xml", "Album F", "Artist F");
    f.tracks = json!([{
        "track_guid": "track-f1",
        "title": "Track F1",
        "pub_date": 1_700_000_000,
        "explicit": false,
        "payment_routes": [],
        "value_time_splits": [],
        "remote_items": [remote_item(0, "publisher", "publisher-t", "https://t.example/t.xml")],
    }]);
    ingest(&st, &f).await;

    let body = get(
        &st,
        "/v1/feeds/album-f/tracks/track-f1?include=remote_items",
    )
    .await;
    let entry = find(
        entries(&body, "remote_items"),
        "remote_feed_guid",
        "publisher-t",
    );
    assert_eq!(
        entry["remote_feed_title"], "Publisher T",
        "ADR 0059 §1: a track read gives the title of each indexed named feed"
    );
    assert_eq!(
        entry["remote_release_artist"], "Label T",
        "ADR 0059 §1: a track read gives the artist of each indexed named feed"
    );
}

/// ADR 0059 §4: the read of a large publisher, measured before and after the
/// change. Run with
/// `cargo test --release --test adr0059_entry_summary_tests -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "timing measurement for ADR 0059 §4, not a gate"]
async fn measure_a_publisher_read_with_150_albums() {
    const ALBUMS: usize = 150;
    const READS: usize = 20;

    let st = state(common::test_db_arc());
    let mut listed = Vec::new();
    for i in 0..ALBUMS {
        let guid = format!("album-{i:03}");
        let url = format!("https://albums.example/{i:03}.xml");
        let title = format!("Album {i:03}");
        ingest(&st, &album(&guid, &url, &title, "Timing Artist")).await;
        listed.push(remote_item(
            i64::try_from(i).expect("small index"),
            "music",
            &guid,
            &url,
        ));
    }
    ingest(
        &st,
        &Feed {
            guid: "publisher-big",
            url: "https://big.example/big.xml",
            title: "Publisher Big",
            medium: "publisher",
            image_url: None,
            author: None,
            remote_items: Value::Array(listed),
            tracks: json!([]),
        },
    )
    .await;

    let mut times: Vec<Duration> = Vec::with_capacity(READS);
    for _ in 0..READS {
        let start = Instant::now();
        let body = get(&st, "/v1/feeds/publisher-big?include=publisher").await;
        times.push(start.elapsed());
        assert_eq!(
            entries(&body, "publisher").len(),
            ALBUMS,
            "the publisher lists each album"
        );
    }
    times.sort();
    println!(
        "ADR 0059 §4: publisher read with {ALBUMS} albums, median of {READS}: {:?}",
        times[READS / 2]
    );
}
