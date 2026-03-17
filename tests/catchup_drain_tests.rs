// Issue-CATCHUP-DRAIN — 2026-03-15
//
// Verify that the community poll loop drains all available pages before
// sleeping, so that a node 20 000 events behind converges in seconds
// rather than hours (40 sleep cycles).

#![expect(
    clippy::significant_drop_tightening,
    reason = "MutexGuard<Connection> must be held for the full scope in test assertions"
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicI64;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Build a signed `ArtistUpserted` event JSON value for wiremock responses.
fn make_event_json(
    signer: &stophammer::signing::NodeSigner,
    event_id: &str,
    seq: i64,
) -> serde_json::Value {
    let now = stophammer::db::unix_now();
    let artist_payload = serde_json::json!({
        "artist": {
            "artist_id": format!("art-{event_id}"),
            "name": format!("Artist {event_id}"),
            "name_lower": format!("artist {event_id}"),
            "created_at": now,
            "updated_at": now
        }
    });
    let payload_json = serde_json::to_string(&artist_payload).unwrap();

    let (signed_by, signature) = signer.sign_event(
        event_id,
        &stophammer::event::EventType::ArtistUpserted,
        &payload_json,
        &format!("art-{event_id}"),
        now,
        seq,
    );

    serde_json::json!({
        "event_id": event_id,
        "event_type": "artist_upserted",
        "payload": { "type": "artist_upserted", "data": artist_payload },
        "subject_guid": format!("art-{event_id}"),
        "signed_by": signed_by,
        "signature": signature,
        "seq": seq,
        "created_at": now,
        "warnings": [],
        "payload_json": payload_json
    })
}

/// A wiremock responder that returns different pages based on the `after_seq`
/// query parameter, simulating a primary with multiple pages of events.
struct PagedResponder {
    page1: serde_json::Value,
    page2: serde_json::Value,
    page3: serde_json::Value,
}

impl Respond for PagedResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let query = request.url.query().unwrap_or_default();
        // Parse after_seq from query string
        let after_seq: i64 = query
            .split('&')
            .find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                if k == "after_seq" {
                    v.parse().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let body = match after_seq {
            0 => &self.page1,
            3 => &self.page2,
            _ => &self.page3,
        };
        ResponseTemplate::new(200).set_body_json(body)
    }
}

// ---------------------------------------------------------------------------
// The community drain loop must fetch all available pages before sleeping.
//
// Mock primary returns 3 pages:
//   page 1 (after_seq=0):  events seq 1..3,   has_more=true,  next_seq=3
//   page 2 (after_seq=3):  events seq 4..6,   has_more=true,  next_seq=6
//   page 3 (after_seq=6):  events seq 7..9,   has_more=false, next_seq=9
//
// After one drain cycle the cursor must be at 9 and all 3 pages must have
// been requested.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn drain_loop_fetches_all_pages_before_sleeping() {
    let mock_server = MockServer::start().await;
    let signer = stophammer::signing::NodeSigner::load_or_create("/tmp/test-drain-signer.key")
        .expect("create signer");

    // Page 1: after_seq=0
    let page1_events: Vec<serde_json::Value> = (1..=3_i64)
        .map(|seq| make_event_json(&signer, &format!("drain-evt-{seq}"), seq))
        .collect();

    // Page 2: after_seq=3
    let page2_events: Vec<serde_json::Value> = (4..=6_i64)
        .map(|seq| make_event_json(&signer, &format!("drain-evt-{seq}"), seq))
        .collect();

    // Page 3: after_seq=6
    let page3_events: Vec<serde_json::Value> = (7..=9_i64)
        .map(|seq| make_event_json(&signer, &format!("drain-evt-{seq}"), seq))
        .collect();

    let responder = PagedResponder {
        page1: serde_json::json!({ "events": page1_events, "has_more": true,  "next_seq": 3 }),
        page2: serde_json::json!({ "events": page2_events, "has_more": true,  "next_seq": 6 }),
        page3: serde_json::json!({ "events": page3_events, "has_more": false, "next_seq": 9 }),
    };

    Mock::given(method("GET"))
        .and(path("/sync/events"))
        .respond_with(responder)
        .mount(&mock_server)
        .await;

    // Mount tracker and register stubs.
    Mock::given(method("POST"))
        .and(path("/nodes/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/sync/register"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let db = common::test_db_arc();
    let pool = common::wrap_pool(db.clone());
    let last_push_at = Arc::new(AtomicI64::new(0));

    let config = stophammer::community::CommunityConfig {
        primary_url: mock_server.uri(),
        tracker_url: mock_server.uri(),
        node_address: "http://localhost:9999".into(),
        poll_interval_secs: 300, // long sleep — if drain works we finish in <1s
        push_timeout_secs: 0,    // immediately trigger poll
    };

    let db2 = pool.clone();
    let lp = Arc::clone(&last_push_at);

    let handle = tokio::spawn(async move {
        stophammer::community::run_community_sync(config, db2, "deadbeef-drain".into(), lp, None)
            .await;
    });

    // Give the drain loop enough time to fetch all 3 pages.
    // With the fix it should complete in milliseconds; without the fix it
    // would sleep 300s between pages and never finish.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    handle.abort();
    let _ = handle.await;

    // Assert: cursor must have advanced past all 3 pages.
    {
        let conn = db.lock().expect("lock for cursor check");
        let cursor: i64 = conn
            .query_row(
                "SELECT COALESCE(\
                    (SELECT last_seq FROM node_sync_state WHERE node_pubkey = 'primary_sync_cursor'), 0)",
                [],
                |row| row.get(0),
            )
            .expect("query cursor");

        assert_eq!(
            cursor, 9,
            "cursor must advance to 9 after draining all 3 pages"
        );

        // Assert: all 9 events should be in the database.
        let event_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .expect("query event count");
        assert_eq!(event_count, 9, "all 9 events from 3 pages must be applied");
    }

    // Assert: verify the mock received exactly 3 GET /sync/events requests
    // (one per page, no extra requests from unnecessary re-polls).
    let received = mock_server.received_requests().await.unwrap();
    let sync_gets = received
        .iter()
        .filter(|r| r.method == wiremock::http::Method::GET && r.url.path() == "/sync/events")
        .count();
    assert_eq!(
        sync_gets, 3,
        "exactly 3 pages should be fetched in one drain cycle"
    );
}
