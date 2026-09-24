//! ADR 0048: the V4V gate is track coverage, not a feed-level block.
//!
//! Each test changes a copy of the real `detox-album` fixture (ADR 0049 task
//! 002), which has one track and a valid route on the feed and on the track.
//! Each test calls the real `V4VPaymentVerifier`.

mod common;

use serde_json::{Value, json};
use stophammer::ingest::IngestFeedRequest;
use stophammer::verifiers::v4v_payment::V4VPaymentVerifier;
use stophammer::verify::{IngestContext, Verifier, VerifyResult};

fn detox_album() -> Value {
    common::adr0049_feed_data("detox-album")
}

fn valid_route() -> Value {
    json!({
        "recipient_name": "Artist",
        "route_type": "node",
        "address": "02682b7c86f474d082fa9d274c3751291225448468691784c6f112187de975a8c2",
        "custom_key": null,
        "custom_value": null,
        "split": 100,
        "fee": false
    })
}

fn invalid_route() -> Value {
    json!({
        "recipient_name": "Nobody",
        "route_type": "node",
        "address": "",
        "custom_key": null,
        "custom_value": null,
        "split": 0,
        "fee": false
    })
}

fn verify(feed_data: &Value) -> VerifyResult {
    let request: IngestFeedRequest = serde_json::from_value(json!({
        "canonical_url": "https://example.com/feed.xml",
        "source_url": "https://example.com/feed.xml",
        "crawl_token": "test-token",
        "http_status": 200,
        "content_hash": "adr0048",
        "feed_data": feed_data
    }))
    .expect("ingest request decodes");
    let conn = common::test_db();
    let ctx = IngestContext {
        request: &request,
        db: &conn,
        existing: None,
    };
    V4VPaymentVerifier.verify(&ctx)
}

fn track_guid(feed: &Value) -> String {
    feed["tracks"][0]["track_guid"]
        .as_str()
        .expect("fixture track has a guid")
        .to_string()
}

#[test]
fn the_fixture_passes_unchanged() {
    assert!(
        matches!(verify(&detox_album()), VerifyResult::Pass),
        "the detox-album fixture has a valid feed route and a valid track route"
    );
}

#[test]
fn tracks_that_each_carry_a_route_pass_with_no_feed_level_block() {
    let mut feed = detox_album();
    feed["feed_payment_routes"] = json!([]);
    feed["tracks"][0]["payment_routes"] = json!([valid_route()]);

    assert!(
        matches!(verify(&feed), VerifyResult::Pass),
        "ADR 0048: a feed needs a feed-level block only when a track declares none"
    );
}

#[test]
fn a_track_with_no_routes_and_no_feed_level_block_is_refused_by_name() {
    let mut feed = detox_album();
    feed["feed_payment_routes"] = json!([]);
    feed["tracks"][0]["payment_routes"] = json!([]);
    let guid = track_guid(&feed);

    match verify(&feed) {
        VerifyResult::Fail(msg) => assert!(
            msg.contains(&guid),
            "ADR 0048: the failure must name the track {guid}, got: {msg}"
        ),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_valid_feed_block_and_an_invalid_track_block_is_refused() {
    let mut feed = detox_album();
    feed["feed_payment_routes"] = json!([valid_route()]);
    feed["tracks"][0]["payment_routes"] = json!([invalid_route()]);
    let guid = track_guid(&feed);

    match verify(&feed) {
        VerifyResult::Fail(msg) => assert!(
            msg.contains(&guid),
            "a declared track block must hold a valid recipient, got: {msg}"
        ),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_feed_with_no_tracks_and_no_feed_level_block_is_refused() {
    let mut feed = detox_album();
    feed["feed_payment_routes"] = json!([]);
    feed["tracks"] = json!([]);

    assert!(
        matches!(verify(&feed), VerifyResult::Fail(_)),
        "ADR 0048 rule 4: a music feed with no track and no feed block does not participate"
    );
}

#[test]
fn a_track_with_no_routes_falls_back_to_a_valid_feed_block() {
    let mut feed = detox_album();
    feed["feed_payment_routes"] = json!([valid_route()]);
    feed["tracks"][0]["payment_routes"] = json!([]);

    assert!(
        matches!(verify(&feed), VerifyResult::Pass),
        "a track with no routes of its own is covered by the feed-level routes"
    );
}

#[test]
fn an_invalid_feed_block_is_refused_even_when_each_track_is_covered() {
    let mut feed = detox_album();
    feed["feed_payment_routes"] = json!([invalid_route()]);
    feed["tracks"][0]["payment_routes"] = json!([valid_route()]);

    assert!(
        matches!(verify(&feed), VerifyResult::Fail(_)),
        "a feed-level block that the feed declares must hold a valid recipient"
    );
}

#[test]
fn a_publisher_feed_stays_exempt() {
    let mut feed = detox_album();
    feed["raw_medium"] = json!("publisher");
    feed["feed_payment_routes"] = json!([]);
    feed["tracks"][0]["payment_routes"] = json!([]);

    assert!(
        matches!(verify(&feed), VerifyResult::Pass),
        "ADR 0048 rule 5: the publisher and musicL exemption is unchanged"
    );
}
