// ADR 0061: a publisher read counts its confirmed artists.
//
// docs/tasks/adr-0061-task-001-confirmed-artists.md
//
// A feed read of a publisher feed gives `confirmed_release_artists`,
// `confirmed_release_artist_count`, `unconfirmed_release_artists` and
// `unconfirmed_release_artist_count` (ADR 0061 §1). Each test below is one
// guard of ADR 0061, or the guard of ADR 0061 §5 that an album read never
// shows a publisher the album does not name.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const TOKEN: &str = "adr0061-crawl-token";

fn state(db: Arc<Mutex<rusqlite::Connection>>) -> Arc<stophammer::api::AppState> {
    let signer = Arc::new(common::temp_signer("test-adr0061-signer"));
    let pubkey = signer.pubkey_hex().to_string();
    let spec = stophammer::verify::ChainSpec { names: vec![] };
    let chain = stophammer::verify::build_chain(&spec, TOKEN.to_string());
    Arc::new(stophammer::api::AppState {
        db: stophammer::db_pool::DbPool::from_writer_only(db),
        chain: Arc::new(chain),
        signer,
        node_pubkey_hex: pubkey,
        admin_token: "test-adr0061-admin-token".into(),
        sync_token: None,
        push_client: reqwest::Client::new(),
        push_subscribers: Arc::new(RwLock::new(HashMap::new())),
        sse_registry: Arc::new(stophammer::api::SseRegistry::new()),
        source_gone_hosts: Vec::new(),
        skip_ssrf_validation: true,
    })
}

/// One feed to ingest.
struct Feed<'a> {
    guid: &'a str,
    url: &'a str,
    title: &'a str,
    medium: &'a str,
    author: Option<&'a str>,
    remote_items: Value,
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
            "author_name": feed.author,
            "remote_items": feed.remote_items,
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

/// An album that lists `publisher_guid`/`publisher_url` back with
/// `medium="publisher"`, so it names the publisher (a confirmed link).
fn confirming_album<'a>(
    guid: &'a str,
    url: &'a str,
    title: &'a str,
    author: &'a str,
    publisher_guid: &'a str,
    publisher_url: &'a str,
) -> Feed<'a> {
    Feed {
        guid,
        url,
        title,
        medium: "music",
        author: Some(author),
        remote_items: json!([remote_item(0, "publisher", publisher_guid, publisher_url)]),
    }
}

/// An album with no `medium="publisher"` remote item of its own, so it names
/// no publisher (a one-way, unconfirmed link when a publisher lists it).
fn plain_album<'a>(
    guid: &'a str,
    url: &'a str,
    title: &'a str,
    author: Option<&'a str>,
) -> Feed<'a> {
    Feed {
        guid,
        url,
        title,
        medium: "music",
        author,
        remote_items: json!([]),
    }
}

fn publisher<'a>(guid: &'a str, url: &'a str, title: &'a str, listed: Value) -> Feed<'a> {
    Feed {
        guid,
        url,
        title,
        medium: "publisher",
        author: None,
        remote_items: listed,
    }
}

/// A publisher that lists 2 albums by 2 artists, of which no album names it,
/// gives `confirmed_release_artist_count` 0 and
/// `unconfirmed_release_artist_count` 2.
#[tokio::test]
async fn no_confirming_album_gives_zero_confirmed_and_two_unconfirmed() {
    let st = state(common::test_db_arc());
    let p_guid = "p-none-confirm";
    let p_url = "https://p.example/none-confirm.xml";

    ingest(
        &st,
        &plain_album(
            "album-one",
            "https://a.example/one.xml",
            "Album One",
            Some("Artist One"),
        ),
    )
    .await;
    ingest(
        &st,
        &plain_album(
            "album-two",
            "https://a.example/two.xml",
            "Album Two",
            Some("Artist Two"),
        ),
    )
    .await;
    ingest(
        &st,
        &publisher(
            p_guid,
            p_url,
            "Publisher None Confirm",
            json!([
                remote_item(0, "music", "album-one", "https://a.example/one.xml"),
                remote_item(1, "music", "album-two", "https://a.example/two.xml"),
            ]),
        ),
    )
    .await;

    let body = get(&st, &format!("/v1/feeds/{p_guid}")).await;
    assert_eq!(
        body["data"]["confirmed_release_artist_count"],
        json!(0),
        "no listed album names the publisher back: {body:?}"
    );
    assert_eq!(
        body["data"]["confirmed_release_artists"],
        json!([]),
        "the confirmed list must be empty: {body:?}"
    );
    assert_eq!(
        body["data"]["unconfirmed_release_artist_count"],
        json!(2),
        "both listed albums must count as unconfirmed: {body:?}"
    );
    assert_eq!(
        body["data"]["unconfirmed_release_artists"],
        json!(["Artist One", "Artist Two"]),
        "the unconfirmed list must give each raw artist, in listing order: {body:?}"
    );
}

/// A publisher that lists 2 albums by 2 artists, of which one names it,
/// gives 1 and 1.
#[tokio::test]
async fn one_confirming_album_gives_one_confirmed_and_one_unconfirmed() {
    let st = state(common::test_db_arc());
    let p_guid = "p-one-confirm";
    let p_url = "https://p.example/one-confirm.xml";

    ingest(
        &st,
        &confirming_album(
            "album-confirms",
            "https://a.example/confirms.xml",
            "Album Confirms",
            "Confirming Artist",
            p_guid,
            p_url,
        ),
    )
    .await;
    ingest(
        &st,
        &plain_album(
            "album-silent",
            "https://a.example/silent.xml",
            "Album Silent",
            Some("Silent Artist"),
        ),
    )
    .await;
    ingest(
        &st,
        &publisher(
            p_guid,
            p_url,
            "Publisher One Confirm",
            json!([
                remote_item(
                    0,
                    "music",
                    "album-confirms",
                    "https://a.example/confirms.xml"
                ),
                remote_item(1, "music", "album-silent", "https://a.example/silent.xml"),
            ]),
        ),
    )
    .await;

    let body = get(&st, &format!("/v1/feeds/{p_guid}")).await;
    assert_eq!(
        body["data"]["confirmed_release_artist_count"],
        json!(1),
        "the album that names the publisher back must be confirmed: {body:?}"
    );
    assert_eq!(
        body["data"]["confirmed_release_artists"],
        json!(["Confirming Artist"]),
        "{body:?}"
    );
    assert_eq!(
        body["data"]["unconfirmed_release_artist_count"],
        json!(1),
        "the album that does not name the publisher back must be unconfirmed: {body:?}"
    );
    assert_eq!(
        body["data"]["unconfirmed_release_artists"],
        json!(["Silent Artist"]),
        "{body:?}"
    );
}

/// A listed album with a `placeholder` artist is not counted.
///
/// The album declares no `author_name` and no `owner_name`, so
/// `derive_release_artist` gives it the placeholder value "Unknown Artist"
/// with source "placeholder" (`src/api.rs`). The album still names the
/// publisher back, with `medium="publisher"`, so the only reason it must not
/// count is the placeholder source.
#[tokio::test]
async fn placeholder_artist_album_is_not_counted() {
    let st = state(common::test_db_arc());
    let p_guid = "p-placeholder";
    let p_url = "https://p.example/placeholder.xml";
    let album_guid = "album-placeholder";
    let album_url = "https://a.example/placeholder.xml";

    ingest(
        &st,
        &Feed {
            guid: album_guid,
            url: album_url,
            title: "Album Placeholder",
            medium: "music",
            author: None,
            remote_items: json!([remote_item(0, "publisher", p_guid, p_url)]),
        },
    )
    .await;
    ingest(
        &st,
        &publisher(
            p_guid,
            p_url,
            "Publisher Placeholder",
            json!([remote_item(0, "music", album_guid, album_url)]),
        ),
    )
    .await;

    let body = get(&st, &format!("/v1/feeds/{p_guid}")).await;
    assert_eq!(
        body["data"]["confirmed_release_artist_count"],
        json!(0),
        "a placeholder release_artist must not be counted, even on a two-way link: {body:?}"
    );
    assert_eq!(
        body["data"]["confirmed_release_artists"],
        json!([]),
        "{body:?}"
    );
}

/// Two listed albums by "Ann Lee" and " ann  lee " count as one artist.
#[tokio::test]
async fn normalization_dedupes_case_and_whitespace_variants() {
    let st = state(common::test_db_arc());
    let p_guid = "p-normalize";
    let p_url = "https://p.example/normalize.xml";

    ingest(
        &st,
        &confirming_album(
            "album-ann-lee",
            "https://a.example/ann-lee.xml",
            "Album Ann Lee",
            "Ann Lee",
            p_guid,
            p_url,
        ),
    )
    .await;
    ingest(
        &st,
        &confirming_album(
            "album-ann-lee-spaced",
            "https://a.example/ann-lee-spaced.xml",
            "Album Ann Lee Spaced",
            " ann  lee ",
            p_guid,
            p_url,
        ),
    )
    .await;
    ingest(
        &st,
        &publisher(
            p_guid,
            p_url,
            "Publisher Normalize",
            json!([
                remote_item(0, "music", "album-ann-lee", "https://a.example/ann-lee.xml"),
                remote_item(
                    1,
                    "music",
                    "album-ann-lee-spaced",
                    "https://a.example/ann-lee-spaced.xml"
                ),
            ]),
        ),
    )
    .await;

    let body = get(&st, &format!("/v1/feeds/{p_guid}")).await;
    assert_eq!(
        body["data"]["confirmed_release_artist_count"],
        json!(1),
        "\"Ann Lee\" and \" ann  lee \" must normalize to one artist: {body:?}"
    );
    assert_eq!(
        body["data"]["confirmed_release_artists"],
        json!(["Ann Lee"]),
        "the kept raw value must be the first row's: {body:?}"
    );
}

/// An unresolved listed album is not counted.
#[tokio::test]
async fn unresolved_listed_album_is_not_counted() {
    let st = state(common::test_db_arc());
    let p_guid = "p-unresolved";
    let p_url = "https://p.example/unresolved.xml";

    ingest(
        &st,
        &publisher(
            p_guid,
            p_url,
            "Publisher Unresolved",
            json!([remote_item(
                0,
                "music",
                "album-never-ingested",
                "https://a.example/never-ingested.xml"
            )]),
        ),
    )
    .await;

    let body = get(&st, &format!("/v1/feeds/{p_guid}")).await;
    assert_eq!(
        body["data"]["confirmed_release_artist_count"],
        json!(0),
        "an unresolved listed album must not count as confirmed: {body:?}"
    );
    assert_eq!(
        body["data"]["unconfirmed_release_artist_count"],
        json!(0),
        "an unresolved listed album must not count as unconfirmed: {body:?}"
    );
    assert_eq!(
        body["data"]["confirmed_release_artists"],
        json!([]),
        "{body:?}"
    );
    assert_eq!(
        body["data"]["unconfirmed_release_artists"],
        json!([]),
        "{body:?}"
    );
}

/// ADR 0061 §5: the read of an album that a publisher lists, and that does
/// not name the publisher, gives no `publisher` row for that publisher.
#[tokio::test]
async fn album_read_gives_no_publisher_row_for_a_publisher_it_does_not_name() {
    let st = state(common::test_db_arc());
    let p_guid = "p-not-named";
    let p_url = "https://p.example/not-named.xml";
    let album_guid = "album-not-naming";
    let album_url = "https://a.example/not-naming.xml";

    ingest(
        &st,
        &plain_album(album_guid, album_url, "Album Not Naming", Some("Artist")),
    )
    .await;
    ingest(
        &st,
        &publisher(
            p_guid,
            p_url,
            "Publisher Not Named",
            json!([remote_item(0, "music", album_guid, album_url)]),
        ),
    )
    .await;

    // The publisher's own read still gives the link, in `unconfirmed_*` and
    // in its own `publisher` view (not asserted here beyond the count).
    let publisher_body = get(&st, &format!("/v1/feeds/{p_guid}")).await;
    assert_eq!(
        publisher_body["data"]["unconfirmed_release_artist_count"],
        json!(1),
        "the publisher side must still see the one-way link: {publisher_body:?}"
    );

    // The album's own read must give no `publisher` row for `p_guid`: the
    // album's own remote_items name no publisher.
    let album_body = get(&st, &format!("/v1/feeds/{album_guid}?include=publisher")).await;
    let rows = album_body["data"]["publisher"]
        .as_array()
        .expect("the album read must give a `publisher` array");
    assert!(
        rows.is_empty(),
        "an album that names no publisher must give no `publisher` row, \
         even though a publisher lists it: {album_body:?}"
    );
}

/// A music feed read has none of the four fields: the keys are absent, not
/// null.
#[tokio::test]
async fn music_feed_read_has_none_of_the_four_fields() {
    let st = state(common::test_db_arc());
    let feed_guid = "plain-music-adr0061";
    let feed_url = "https://a.example/plain-music-adr0061.xml";

    ingest(
        &st,
        &plain_album(feed_guid, feed_url, "Plain Music Feed", Some("Some Artist")),
    )
    .await;

    let body = get(&st, &format!("/v1/feeds/{feed_guid}")).await;
    for key in [
        "confirmed_release_artist_count",
        "confirmed_release_artists",
        "unconfirmed_release_artist_count",
        "unconfirmed_release_artists",
    ] {
        assert!(
            body["data"].get(key).is_none(),
            "a music feed read must not carry the key {key}: {body:?}"
        );
    }
}
