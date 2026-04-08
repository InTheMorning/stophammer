#![allow(
    clippy::clone_on_ref_ptr,
    reason = "tests use Arc-backed in-memory DB handles; Arc::clone adds noise without changing intent"
)]

mod common;

use rusqlite::params;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn insert_artist(conn: &rusqlite::Connection, artist_id: &str, name: &str, now: i64) {
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artist_id, name, name.to_lowercase(), now, now],
    )
    .unwrap();
}

fn insert_artist_credit(
    conn: &rusqlite::Connection,
    artist_id: &str,
    display_name: &str,
    now: i64,
) -> i64 {
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES (?1, ?2)",
        params![display_name, now],
    )
    .unwrap();
    let credit_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![credit_id, artist_id, 0, display_name, ""],
    )
    .unwrap();

    credit_id
}

fn insert_feed(
    conn: &rusqlite::Connection,
    feed_guid: &str,
    feed_url: &str,
    title: &str,
    credit_id: i64,
    now: i64,
) {
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            feed_guid,
            feed_url,
            title,
            title.to_lowercase(),
            credit_id,
            "A test feed",
            0,
            0,
            now,
            now,
        ],
    )
    .unwrap();
}

fn insert_track(
    conn: &rusqlite::Connection,
    track_guid: &str,
    feed_guid: &str,
    credit_id: i64,
    title: &str,
    now: i64,
) {
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         description, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            track_guid,
            feed_guid,
            credit_id,
            title,
            title.to_lowercase(),
            "A test track",
            0,
            now,
            now,
        ],
    )
    .unwrap();
}

fn insert_payment_route(conn: &rusqlite::Connection, track_guid: &str, feed_guid: &str) {
    conn.execute(
        "INSERT INTO payment_routes (track_guid, feed_guid, recipient_name, route_type, \
         address, split, fee) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            track_guid,
            feed_guid,
            "Artist Wallet",
            "node",
            "some-address",
            95,
            0
        ],
    )
    .unwrap();
}

fn insert_feed_payment_route(conn: &rusqlite::Connection, feed_guid: &str) {
    conn.execute(
        "INSERT INTO feed_payment_routes (feed_guid, recipient_name, route_type, address, split, fee) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![feed_guid, "Feed Wallet", "lnaddress", "feed@pay.com", 100, 0],
    )
    .unwrap();
}

fn insert_value_time_split(conn: &rusqlite::Connection, track_guid: &str, now: i64) {
    conn.execute(
        "INSERT INTO value_time_splits (source_track_guid, start_time_secs, duration_secs, \
         remote_feed_guid, remote_item_guid, split, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            track_guid,
            30,
            60,
            "remote-feed-1",
            "remote-item-1",
            50,
            now
        ],
    )
    .unwrap();
}

fn insert_entity_quality(
    conn: &rusqlite::Connection,
    entity_type: &str,
    entity_id: &str,
    now: i64,
) {
    conn.execute(
        "INSERT INTO entity_quality (entity_type, entity_id, score, computed_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![entity_type, entity_id, 80, now],
    )
    .unwrap();
}

fn insert_track_tag(conn: &rusqlite::Connection, track_guid: &str, now: i64) -> i64 {
    conn.execute(
        "INSERT OR IGNORE INTO tags (name, created_at) VALUES ('rock', ?1)",
        params![now],
    )
    .unwrap();
    let tag_id: i64 = conn
        .query_row("SELECT id FROM tags WHERE name = 'rock'", [], |r| r.get(0))
        .unwrap();
    conn.execute(
        "INSERT INTO track_tag (track_guid, tag_id, created_at) VALUES (?1, ?2, ?3)",
        params![track_guid, tag_id, now],
    )
    .unwrap();
    tag_id
}

fn insert_feed_tag(conn: &rusqlite::Connection, feed_guid: &str, now: i64) -> i64 {
    conn.execute(
        "INSERT OR IGNORE INTO tags (name, created_at) VALUES ('electronic', ?1)",
        params![now],
    )
    .unwrap();
    let tag_id: i64 = conn
        .query_row("SELECT id FROM tags WHERE name = 'electronic'", [], |r| {
            r.get(0)
        })
        .unwrap();
    conn.execute(
        "INSERT INTO feed_tag (feed_guid, tag_id, created_at) VALUES (?1, ?2, ?3)",
        params![feed_guid, tag_id, now],
    )
    .unwrap();
    tag_id
}

fn count(conn: &rusqlite::Connection, table: &str, where_clause: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {where_clause}");
    conn.query_row(&sql, [], |r| r.get(0)).unwrap()
}

/// Populate a feed with two tracks and all associated child rows
/// (payment routes, VTS, tags, quality).
fn populate_feed_with_tracks(conn: &rusqlite::Connection) -> i64 {
    let now = common::now();
    insert_artist(conn, "artist-1", "Test Artist", now);
    let credit_id = insert_artist_credit(conn, "artist-1", "Test Artist", now);
    insert_feed(
        conn,
        "feed-1",
        "https://example.com/feed.xml",
        "Test Album",
        credit_id,
        now,
    );

    insert_track(conn, "track-1", "feed-1", credit_id, "Song One", now);
    insert_track(conn, "track-2", "feed-1", credit_id, "Song Two", now);

    // Track-level child rows
    insert_payment_route(conn, "track-1", "feed-1");
    insert_payment_route(conn, "track-2", "feed-1");
    insert_value_time_split(conn, "track-1", now);
    insert_value_time_split(conn, "track-2", now);
    insert_track_tag(conn, "track-1", now);
    insert_track_tag(conn, "track-2", now);
    insert_entity_quality(conn, "track", "track-1", now);
    insert_entity_quality(conn, "track", "track-2", now);

    // Feed-level child rows
    insert_feed_payment_route(conn, "feed-1");
    insert_feed_tag(conn, "feed-1", now);
    insert_entity_quality(conn, "feed", "feed-1", now);

    // Search index rows for feed and tracks
    stophammer::search::populate_search_index(
        conn,
        "feed",
        "feed-1",
        "",
        "Test Album",
        "A test feed",
        "",
    )
    .unwrap();
    stophammer::search::populate_search_index(
        conn,
        "track",
        "track-1",
        "",
        "Song One",
        "A test track",
        "",
    )
    .unwrap();
    stophammer::search::populate_search_index(
        conn,
        "track",
        "track-2",
        "",
        "Song Two",
        "A test track",
        "",
    )
    .unwrap();

    credit_id
}

// ---------------------------------------------------------------------------
// 1. delete_feed removes feed row and all child rows
// ---------------------------------------------------------------------------

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "integration test exercises full feed deletion with all child rows"
)]
fn delete_feed_removes_all_children() {
    let mut conn = common::test_db();
    populate_feed_with_tracks(&conn);

    // Verify data exists before deletion.
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-1'"), 1);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-1'"), 2);
    assert_eq!(count(&conn, "payment_routes", "feed_guid = 'feed-1'"), 2);
    assert_eq!(
        count(
            &conn,
            "value_time_splits",
            "source_track_guid IN ('track-1', 'track-2')"
        ),
        2
    );
    assert_eq!(
        count(&conn, "track_tag", "track_guid IN ('track-1', 'track-2')"),
        2
    );
    assert_eq!(count(&conn, "feed_tag", "feed_guid = 'feed-1'"), 1);
    assert_eq!(
        count(&conn, "feed_payment_routes", "feed_guid = 'feed-1'"),
        1
    );
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_id IN ('feed-1', 'track-1', 'track-2')"
        ),
        3
    );
    // Perform delete.
    stophammer::db::delete_feed(&mut conn, "feed-1").unwrap();

    // Verify everything is gone.
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-1'"), 0);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-1'"), 0);
    assert_eq!(count(&conn, "payment_routes", "feed_guid = 'feed-1'"), 0);
    assert_eq!(
        count(
            &conn,
            "value_time_splits",
            "source_track_guid IN ('track-1', 'track-2')"
        ),
        0
    );
    assert_eq!(
        count(&conn, "track_tag", "track_guid IN ('track-1', 'track-2')"),
        0
    );
    assert_eq!(count(&conn, "feed_tag", "feed_guid = 'feed-1'"), 0);
    assert_eq!(
        count(&conn, "feed_payment_routes", "feed_guid = 'feed-1'"),
        0
    );
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_id IN ('feed-1', 'track-1', 'track-2')"
        ),
        0
    );
    // Artist and artist_credit should still exist (not cascade-deleted).
    assert_eq!(count(&conn, "artists", "artist_id = 'artist-1'"), 1);
}

// ---------------------------------------------------------------------------
// 2. delete_feed is idempotent
// ---------------------------------------------------------------------------

#[test]
fn delete_feed_idempotent() {
    let mut conn = common::test_db();
    populate_feed_with_tracks(&conn);

    stophammer::db::delete_feed(&mut conn, "feed-1").unwrap();
    // Second call should not error.
    stophammer::db::delete_feed(&mut conn, "feed-1").unwrap();

    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-1'"), 0);
}

// ---------------------------------------------------------------------------
// 3. delete_track removes the track row and all child rows
// ---------------------------------------------------------------------------

#[test]
fn delete_track_removes_all_children() {
    let mut conn = common::test_db();
    populate_feed_with_tracks(&conn);

    // Verify track-1 data exists.
    assert_eq!(count(&conn, "tracks", "track_guid = 'track-1'"), 1);
    assert_eq!(count(&conn, "payment_routes", "track_guid = 'track-1'"), 1);
    assert_eq!(
        count(&conn, "value_time_splits", "source_track_guid = 'track-1'"),
        1
    );
    assert_eq!(count(&conn, "track_tag", "track_guid = 'track-1'"), 1);
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_type = 'track' AND entity_id = 'track-1'"
        ),
        1
    );
    // Delete track-1 only.
    stophammer::db::delete_track(&mut conn, "track-1").unwrap();

    // Verify track-1 is gone.
    assert_eq!(count(&conn, "tracks", "track_guid = 'track-1'"), 0);
    assert_eq!(count(&conn, "payment_routes", "track_guid = 'track-1'"), 0);
    assert_eq!(
        count(&conn, "value_time_splits", "source_track_guid = 'track-1'"),
        0
    );
    assert_eq!(count(&conn, "track_tag", "track_guid = 'track-1'"), 0);
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_type = 'track' AND entity_id = 'track-1'"
        ),
        0
    );
    // Verify track-2 is still there.
    assert_eq!(count(&conn, "tracks", "track_guid = 'track-2'"), 1);
    assert_eq!(count(&conn, "payment_routes", "track_guid = 'track-2'"), 1);

    // Verify feed is still there.
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-1'"), 1);
}

// ---------------------------------------------------------------------------
// 4. delete_track is idempotent
// ---------------------------------------------------------------------------

#[test]
fn delete_track_idempotent() {
    let mut conn = common::test_db();
    populate_feed_with_tracks(&conn);

    stophammer::db::delete_track(&mut conn, "track-1").unwrap();
    // Second call should not error.
    stophammer::db::delete_track(&mut conn, "track-1").unwrap();

    assert_eq!(count(&conn, "tracks", "track_guid = 'track-1'"), 0);
}

// ---------------------------------------------------------------------------
// 5a. delete_feed with many tracks removes all children via subqueries
// ---------------------------------------------------------------------------

/// Populate a feed with N tracks, each having payment routes, VTS, tags,
/// and `entity_quality` rows. Returns (`credit_id`, `track_guids`).
fn populate_feed_with_n_tracks(conn: &rusqlite::Connection, n: usize) -> (i64, Vec<String>) {
    let now = common::now();
    insert_artist(conn, "artist-n", "N-Track Artist", now);
    let credit_id = insert_artist_credit(conn, "artist-n", "N-Track Artist", now);
    insert_feed(
        conn,
        "feed-n",
        "https://example.com/feed-n.xml",
        "N-Track Album",
        credit_id,
        now,
    );

    let mut guids = Vec::with_capacity(n);
    for i in 0..n {
        let tg = format!("track-n-{i}");
        let title = format!("Song {i}");
        insert_track(conn, &tg, "feed-n", credit_id, &title, now);
        insert_payment_route(conn, &tg, "feed-n");
        insert_value_time_split(conn, &tg, now);

        // Use a unique tag per track to avoid PK conflicts
        conn.execute(
            "INSERT OR IGNORE INTO tags (name, created_at) VALUES (?1, ?2)",
            params![format!("tag-{i}"), now],
        )
        .expect("insert tag");
        let tag_id: i64 = conn
            .query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![format!("tag-{i}")],
                |r| r.get(0),
            )
            .expect("get tag id");
        conn.execute(
            "INSERT INTO track_tag (track_guid, tag_id, created_at) VALUES (?1, ?2, ?3)",
            params![&tg, tag_id, now],
        )
        .expect("insert track_tag");

        insert_entity_quality(conn, "track", &tg, now);
        guids.push(tg);
    }

    // Feed-level child rows
    insert_feed_payment_route(conn, "feed-n");
    insert_feed_tag(conn, "feed-n", now);
    insert_entity_quality(conn, "feed", "feed-n", now);

    (credit_id, guids)
}

#[expect(
    clippy::too_many_lines,
    reason = "test fixture seeds the full set of feed-scoped dependency tables in one place"
)]
fn seed_delete_feed_with_event_dependents(conn: &rusqlite::Connection, credit_id: i64, now: i64) {
    insert_feed(
        conn,
        "feed-peer",
        "https://example.com/feed-peer.xml",
        "Peer Feed",
        credit_id,
        now,
    );
    insert_track(
        conn,
        "track-delete-extra",
        "feed-n",
        credit_id,
        "Extra Track",
        now,
    );

    conn.execute(
        "INSERT INTO feed_remote_items_raw (
             feed_guid,
             position,
             medium,
             remote_feed_guid,
             remote_feed_url,
             source
         ) VALUES (?1, 0, 'music', 'remote-feed', 'https://remote.example/feed.xml', 'podcast_remote_item')",
        params!["feed-n"],
    )
    .expect("insert feed remote item");
    conn.execute(
        "INSERT INTO live_events (
             live_item_guid,
             feed_guid,
             title,
             content_link,
             status,
             created_at,
             updated_at
         ) VALUES (?1, ?2, 'Live Event', 'https://example.com/live', 'live', ?3, ?3)",
        params!["live-item-n", "feed-n", now],
    )
    .expect("insert live event");
    conn.execute(
        "INSERT INTO source_contributor_claims (
             feed_guid,
             entity_type,
             entity_id,
             position,
             name,
             role,
             role_norm,
             group_name,
             href,
             img,
             source,
             extraction_path,
             observed_at
         ) VALUES (?1, 'feed', ?1, 0, 'N-Track Artist', 'host', 'host', NULL, NULL, NULL, 'itunes:author', 'feed.author', ?2)",
        params!["feed-n", now],
    )
    .expect("insert source contributor claim");
    conn.execute(
        "INSERT INTO source_entity_ids (
             feed_guid,
             entity_type,
             entity_id,
             position,
             scheme,
             value,
             source,
             extraction_path,
             observed_at
         ) VALUES (?1, 'feed', ?1, 0, 'guid', 'feed-n', 'podcast:guid', 'feed.guid', ?2)",
        params!["feed-n", now],
    )
    .expect("insert source entity id");
    conn.execute(
        "INSERT INTO source_entity_links (
             feed_guid,
             entity_type,
             entity_id,
             position,
             link_type,
             url,
             source,
             extraction_path,
             observed_at
         ) VALUES (?1, 'feed', ?1, 0, 'website', 'https://example.com/feed-n', 'atom:link', 'feed.link', ?2)",
        params!["feed-n", now],
    )
    .expect("insert source entity link");
    conn.execute(
        "INSERT INTO source_release_claims (
             feed_guid,
             entity_type,
             entity_id,
             position,
             claim_type,
             claim_value,
             source,
             extraction_path,
             observed_at
         ) VALUES (?1, 'feed', ?1, 0, 'release_date', '2026-01-01', 'itunes:date', 'feed.pub_date', ?2)",
        params!["feed-n", now],
    )
    .expect("insert source release claim");
    conn.execute(
        "INSERT INTO source_item_enclosures (
             feed_guid,
             entity_type,
             entity_id,
             position,
             url,
             mime_type,
             bytes,
             rel,
             title,
             is_primary,
             source,
             extraction_path,
             observed_at
         ) VALUES (?1, 'track', 'track-delete-extra', 0, 'https://cdn.example/track.mp3', 'audio/mpeg', 123, 'enclosure', 'Track', 1, 'enclosure', 'track.enclosure', ?2)",
        params!["feed-n", now],
    )
    .expect("insert source item enclosure");
    conn.execute(
        "INSERT INTO source_platform_claims (
             feed_guid,
             platform_key,
             url,
             owner_name,
             source,
             extraction_path,
             observed_at
         ) VALUES (?1, 'wavlake', 'https://wavlake.com/album/test', 'N-Track Artist', 'podcast:value', 'feed.value', ?2)",
        params!["feed-n", now],
    )
    .expect("insert source platform claim");
    conn.execute(
        "INSERT INTO releases (
             release_id,
             title,
             title_lower,
             artist_credit_id,
             created_at,
             updated_at
         ) VALUES (?1, 'Test Release', 'test release', ?2, ?3, ?3)",
        params!["release-delete-n", credit_id, now],
    )
    .expect("insert release");
    conn.execute(
        "INSERT INTO recordings (
             recording_id,
             title,
             title_lower,
             artist_credit_id,
             created_at,
             updated_at
         ) VALUES (?1, 'Test Recording', 'test recording', ?2, ?3, ?3)",
        params!["recording-delete-n", credit_id, now],
    )
    .expect("insert recording");
    conn.execute(
        "INSERT INTO source_feed_release_map (
             feed_guid,
             release_id,
             match_type,
             confidence,
             created_at
         ) VALUES (?1, 'release-delete-n', 'test', 100, ?2)",
        params!["feed-n", now],
    )
    .expect("insert source feed release map");
    conn.execute(
        "INSERT INTO source_item_recording_map (
             track_guid,
             recording_id,
             match_type,
             confidence,
             created_at
         ) VALUES ('track-delete-extra', 'recording-delete-n', 'test', 100, ?1)",
        params![now],
    )
    .expect("insert source item recording map");
}

#[test]
fn delete_feed_many_tracks_removes_all_children() {
    let mut conn = common::test_db();
    let (_credit_id, _track_guids) = populate_feed_with_n_tracks(&conn, 5);

    // Verify data exists before deletion.
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-n'"), 1);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-n'"), 5);
    assert_eq!(count(&conn, "payment_routes", "feed_guid = 'feed-n'"), 5);
    assert_eq!(
        count(
            &conn,
            "value_time_splits",
            "source_track_guid LIKE 'track-n-%'"
        ),
        5
    );
    assert_eq!(count(&conn, "track_tag", "track_guid LIKE 'track-n-%'"), 5);
    assert_eq!(count(&conn, "feed_tag", "feed_guid = 'feed-n'"), 1);
    assert_eq!(
        count(&conn, "feed_payment_routes", "feed_guid = 'feed-n'"),
        1
    );
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_id LIKE 'track-n-%' OR entity_id = 'feed-n'"
        ),
        6
    );
    // Perform delete.
    stophammer::db::delete_feed(&mut conn, "feed-n").expect("delete_feed failed");

    // Verify everything is gone.
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-n'"), 0);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-n'"), 0);
    assert_eq!(count(&conn, "payment_routes", "feed_guid = 'feed-n'"), 0);
    assert_eq!(
        count(
            &conn,
            "value_time_splits",
            "source_track_guid LIKE 'track-n-%'"
        ),
        0
    );
    assert_eq!(count(&conn, "track_tag", "track_guid LIKE 'track-n-%'"), 0);
    assert_eq!(count(&conn, "feed_tag", "feed_guid = 'feed-n'"), 0);
    assert_eq!(
        count(&conn, "feed_payment_routes", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_id LIKE 'track-n-%' OR entity_id = 'feed-n'"
        ),
        0
    );
    // Artist should still exist.
    assert_eq!(count(&conn, "artists", "artist_id = 'artist-n'"), 1);
}

#[test]
fn delete_feed_with_event_many_tracks_removes_all_children() {
    let mut conn = common::test_db();
    let (_credit_id, _track_guids) = populate_feed_with_n_tracks(&conn, 5);

    // Verify data exists.
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-n'"), 5);
    assert_eq!(count(&conn, "payment_routes", "feed_guid = 'feed-n'"), 5);
    assert_eq!(
        count(
            &conn,
            "value_time_splits",
            "source_track_guid LIKE 'track-n-%'"
        ),
        5
    );

    let signer = common::temp_signer("test-many-tracks");
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload_json = r#"{"feed_guid":"feed-n","reason":"bulk test"}"#;
    let now = common::now();
    // Issue-SEQ-INTEGRITY — 2026-03-14: signer passed to delete_feed_with_event.
    let (seq, _signed_by, _signature) = stophammer::db::delete_feed_with_event(
        &mut conn,
        "feed-n",
        &event_id,
        payload_json,
        "feed-n",
        &signer,
        now,
        &[],
    )
    .expect("delete_feed_with_event failed");

    assert!(seq > 0);
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-n'"), 0);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-n'"), 0);
    assert_eq!(count(&conn, "payment_routes", "feed_guid = 'feed-n'"), 0);
    assert_eq!(
        count(
            &conn,
            "value_time_splits",
            "source_track_guid LIKE 'track-n-%'"
        ),
        0
    );
    assert_eq!(count(&conn, "track_tag", "track_guid LIKE 'track-n-%'"), 0);
    assert_eq!(
        count(
            &conn,
            "entity_quality",
            "entity_id LIKE 'track-n-%' OR entity_id = 'feed-n'"
        ),
        0
    );
    // Event should be recorded
    assert_eq!(
        count(&conn, "events", &format!("event_id = '{event_id}'")),
        1
    );
}

#[test]
fn delete_feed_with_event_removes_resolver_and_source_dependents() {
    let mut conn = common::test_db();
    let (credit_id, _track_guids) = populate_feed_with_n_tracks(&conn, 2);
    let now = common::now();
    seed_delete_feed_with_event_dependents(&conn, credit_id, now);

    let signer = common::temp_signer("test-delete-dependent-cleanup");
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload_json = r#"{"feed_guid":"feed-n","reason":"cleanup dependents"}"#;

    stophammer::db::delete_feed_with_event(
        &mut conn,
        "feed-n",
        &event_id,
        payload_json,
        "feed-n",
        &signer,
        now,
        &[],
    )
    .expect("delete_feed_with_event should clean resolver/source dependents");

    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-n'"), 0);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-n'"), 0);
    assert_eq!(
        count(&conn, "feed_remote_items_raw", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(count(&conn, "live_events", "feed_guid = 'feed-n'"), 0);
    assert_eq!(
        count(&conn, "source_contributor_claims", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(count(&conn, "source_entity_ids", "feed_guid = 'feed-n'"), 0);
    assert_eq!(
        count(&conn, "source_entity_links", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(
        count(&conn, "source_release_claims", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(
        count(&conn, "source_item_enclosures", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(
        count(&conn, "source_platform_claims", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(
        count(&conn, "source_feed_release_map", "feed_guid = 'feed-n'"),
        0
    );
    assert_eq!(
        count(
            &conn,
            "source_item_recording_map",
            "track_guid = 'track-delete-extra'"
        ),
        0
    );
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-peer'"), 1);
}

// ---------------------------------------------------------------------------
// 5. apply_single_event with FeedRetired deletes the feed and search index
// ---------------------------------------------------------------------------

#[test]
fn apply_feed_retired_event() {
    let conn = common::test_db();
    populate_feed_with_tracks(&conn);

    let db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(db.clone());

    // Build a FeedRetired event.
    let signer = common::temp_signer("test-retire-feed");
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload = stophammer::event::FeedRetiredPayload {
        feed_guid: "feed-1".to_string(),
        reason: Some("testing".to_string()),
    };
    let payload_json = serde_json::to_string(&payload).unwrap();
    let now = common::now();
    let (signed_by, signature) = signer.sign_event(
        &event_id,
        &stophammer::event::EventType::FeedRetired,
        &payload_json,
        "feed-1",
        now,
        0, // Issue-SEQ-INTEGRITY — 2026-03-14
    );

    let tagged = format!(r#"{{"type":"feed_retired","data":{payload_json}}}"#);
    let event_payload: stophammer::event::EventPayload = serde_json::from_str(&tagged).unwrap();

    let event = stophammer::event::Event {
        event_id,
        event_type: stophammer::event::EventType::FeedRetired,
        payload: event_payload,
        payload_json,
        subject_guid: "feed-1".to_string(),
        signed_by,
        signature,
        seq: 0,
        created_at: now,
        warnings: vec![],
    };

    let result = stophammer::apply::apply_single_event(&pool, &event);
    assert!(result.is_ok());

    let conn = db.lock().unwrap();

    // Feed and tracks should be gone.
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-1'"), 0);
    assert_eq!(count(&conn, "tracks", "feed_guid = 'feed-1'"), 0);
}

// ---------------------------------------------------------------------------
// 6. apply_single_event with TrackRemoved deletes the track and search index
// ---------------------------------------------------------------------------

#[test]
fn apply_track_removed_event() {
    let conn = common::test_db();
    populate_feed_with_tracks(&conn);

    let db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(db.clone());

    // Build a TrackRemoved event.
    let signer = common::temp_signer("test-remove-track");
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload = stophammer::event::TrackRemovedPayload {
        track_guid: "track-1".to_string(),
        feed_guid: "feed-1".to_string(),
    };
    let payload_json = serde_json::to_string(&payload).unwrap();
    let now = common::now();
    let (signed_by, signature) = signer.sign_event(
        &event_id,
        &stophammer::event::EventType::TrackRemoved,
        &payload_json,
        "track-1",
        now,
        0, // Issue-SEQ-INTEGRITY — 2026-03-14
    );

    let tagged = format!(r#"{{"type":"track_removed","data":{payload_json}}}"#);
    let event_payload: stophammer::event::EventPayload = serde_json::from_str(&tagged).unwrap();

    let event = stophammer::event::Event {
        event_id,
        event_type: stophammer::event::EventType::TrackRemoved,
        payload: event_payload,
        payload_json,
        subject_guid: "track-1".to_string(),
        signed_by,
        signature,
        seq: 0,
        created_at: now,
        warnings: vec![],
    };

    let result = stophammer::apply::apply_single_event(&pool, &event);
    assert!(result.is_ok());

    let conn = db.lock().unwrap();

    // track-1 should be gone.
    assert_eq!(count(&conn, "tracks", "track_guid = 'track-1'"), 0);
    // track-2 and feed should remain.
    assert_eq!(count(&conn, "tracks", "track_guid = 'track-2'"), 1);
    assert_eq!(count(&conn, "feeds", "feed_guid = 'feed-1'"), 1);
}

// ---------------------------------------------------------------------------
// 7. FeedRetired with unknown feed_guid is a no-op
// ---------------------------------------------------------------------------

#[test]
fn apply_feed_retired_unknown_guid_is_noop() {
    let conn = common::test_db();
    let db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(db.clone());

    let signer = common::temp_signer("test-retire-unknown");
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload = stophammer::event::FeedRetiredPayload {
        feed_guid: "nonexistent-feed".to_string(),
        reason: None,
    };
    let payload_json = serde_json::to_string(&payload).unwrap();
    let now = common::now();
    let (signed_by, signature) = signer.sign_event(
        &event_id,
        &stophammer::event::EventType::FeedRetired,
        &payload_json,
        "nonexistent-feed",
        now,
        0, // Issue-SEQ-INTEGRITY — 2026-03-14
    );

    let tagged = format!(r#"{{"type":"feed_retired","data":{payload_json}}}"#);
    let event_payload: stophammer::event::EventPayload = serde_json::from_str(&tagged).unwrap();

    let event = stophammer::event::Event {
        event_id,
        event_type: stophammer::event::EventType::FeedRetired,
        payload: event_payload,
        payload_json,
        subject_guid: "nonexistent-feed".to_string(),
        signed_by,
        signature,
        seq: 0,
        created_at: now,
        warnings: vec![],
    };

    // Should not error even though the feed does not exist.
    let result = stophammer::apply::apply_single_event(&pool, &event);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// 8. TrackRemoved with unknown track_guid is a no-op
// ---------------------------------------------------------------------------

#[test]
fn apply_track_removed_unknown_guid_is_noop() {
    let conn = common::test_db();
    let db = Arc::new(Mutex::new(conn));
    let pool = common::wrap_pool(db.clone());

    let signer = common::temp_signer("test-remove-unknown");
    let event_id = uuid::Uuid::new_v4().to_string();
    let payload = stophammer::event::TrackRemovedPayload {
        track_guid: "nonexistent-track".to_string(),
        feed_guid: "nonexistent-feed".to_string(),
    };
    let payload_json = serde_json::to_string(&payload).unwrap();
    let now = common::now();
    let (signed_by, signature) = signer.sign_event(
        &event_id,
        &stophammer::event::EventType::TrackRemoved,
        &payload_json,
        "nonexistent-track",
        now,
        0, // Issue-SEQ-INTEGRITY — 2026-03-14
    );

    let tagged = format!(r#"{{"type":"track_removed","data":{payload_json}}}"#);
    let event_payload: stophammer::event::EventPayload = serde_json::from_str(&tagged).unwrap();

    let event = stophammer::event::Event {
        event_id,
        event_type: stophammer::event::EventType::TrackRemoved,
        payload: event_payload,
        payload_json,
        subject_guid: "nonexistent-track".to_string(),
        signed_by,
        signature,
        seq: 0,
        created_at: now,
        warnings: vec![],
    };

    // Should not error even though the track does not exist.
    let result = stophammer::apply::apply_single_event(&pool, &event);
    assert!(result.is_ok());
}
