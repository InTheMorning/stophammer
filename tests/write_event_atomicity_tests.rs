// Write + event atomicity tests.

mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Helper: seed the DB with a minimal feed + track for PATCH tests.
// ---------------------------------------------------------------------------

fn seed_feed_and_track(conn: &rusqlite::Connection, now: i64) {
    // Insert an artist.
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, sort_name, type_id, area, \
         img_url, url, begin_year, end_year, created_at, updated_at) \
         VALUES ('art-f2', 'F2 Artist', 'f2 artist', NULL, NULL, NULL, NULL, NULL, NULL, NULL, ?1, ?1)",
        params![now],
    )
    .expect("insert artist");

    // Insert artist_credit.
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES ('F2 Artist', ?1)",
        params![now],
    )
    .expect("insert artist_credit");
    let credit_id: i64 = conn
        .query_row("SELECT last_insert_rowid()", [], |r| r.get(0))
        .expect("get credit id");

    // Insert artist_credit_name.
    conn.execute(
        "INSERT INTO artist_credit_name (artist_credit_id, artist_id, position, name, join_phrase) \
         VALUES (?1, 'art-f2', 0, 'F2 Artist', '')",
        params![credit_id],
    )
    .expect("insert artist_credit_name");

    // Insert a feed.
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, image_url, language, explicit, itunes_type, episode_count, \
         newest_item_at, oldest_item_at, created_at, updated_at, raw_medium) \
         VALUES ('feed-f2', 'https://example.com/original.xml', 'F2 Album', 'f2 album', \
         ?1, 'desc', NULL, 'en', 0, NULL, 1, NULL, NULL, ?2, ?2, 'music')",
        params![credit_id, now],
    )
    .expect("insert feed");

    // Insert a track.
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, artist_credit_id, title, title_lower, \
         pub_date, duration_secs, enclosure_url, enclosure_type, enclosure_bytes, \
         track_number, season, explicit, description, created_at, updated_at) \
         VALUES ('track-f2', 'feed-f2', ?1, 'F2 Track', 'f2 track', \
         ?2, 240, 'https://cdn.example.com/original.mp3', 'audio/mpeg', 5000000, \
         1, NULL, 0, 'desc', ?2, ?2)",
        params![credit_id, now],
    )
    .expect("insert track");
}

// ---------------------------------------------------------------------------
// Test 2: PATCH feed atomicity — rollback on event insert failure.
//
// Directly verifies that UPDATE feeds + INSERT events are atomic: if the
// events table is missing, the feed_url must remain unchanged.
// ---------------------------------------------------------------------------

#[test]
fn patch_feed_inline_rolls_back_when_event_insert_fails() {
    let conn = common::test_db();
    let now = common::now();

    seed_feed_and_track(&conn, now);

    // Verify original feed_url.
    let original_url: String = conn
        .query_row(
            "SELECT feed_url FROM feeds WHERE feed_guid = 'feed-f2'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(original_url, "https://example.com/original.xml");

    // Simulate the same inline transaction pattern used in handle_patch_feed:
    // UPDATE feeds + INSERT events in a single unchecked_transaction.
    // We corrupt the events table to force failure.
    conn.execute_batch("ALTER TABLE events RENAME TO events_backup")
        .expect("rename events table");

    let result: Result<(), rusqlite::Error> = (|| {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE feeds SET feed_url = ?1 WHERE feed_guid = ?2",
            params!["https://example.com/CHANGED.xml", "feed-f2"],
        )?;
        // This INSERT will fail — table doesn't exist.
        tx.execute(
            "INSERT INTO events (event_id, event_type, payload, subject_guid, signed_by, signature, created_at) \
             VALUES ('evt-f2', 'feed.upserted', '{}', 'feed-f2', 'pk', 'sig', ?1)",
            params![now],
        )?;
        tx.commit()
    })();

    // Restore the events table.
    conn.execute_batch("ALTER TABLE events_backup RENAME TO events")
        .expect("restore events table");

    assert!(
        result.is_err(),
        "transaction must fail when events table is missing"
    );

    // The feed_url must be UNCHANGED (UPDATE was rolled back).
    let url_after: String = conn
        .query_row(
            "SELECT feed_url FROM feeds WHERE feed_guid = 'feed-f2'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        url_after, "https://example.com/original.xml",
        "feed_url must be unchanged when event insert failed — UPDATE must be rolled back"
    );
}

// ---------------------------------------------------------------------------
// Test 3: PATCH track atomicity — rollback on event insert failure.
//
// Directly verifies that UPDATE tracks + INSERT events are atomic.
// ---------------------------------------------------------------------------

#[test]
fn patch_track_inline_rolls_back_when_event_insert_fails() {
    let conn = common::test_db();
    let now = common::now();

    seed_feed_and_track(&conn, now);

    // Verify original enclosure_url.
    let original_url: String = conn
        .query_row(
            "SELECT enclosure_url FROM tracks WHERE track_guid = 'track-f2'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(original_url, "https://cdn.example.com/original.mp3");

    // Corrupt the events table.
    conn.execute_batch("ALTER TABLE events RENAME TO events_backup")
        .expect("rename events table");

    let result: Result<(), rusqlite::Error> = (|| {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE tracks SET enclosure_url = ?1 WHERE track_guid = ?2",
            params!["https://cdn.example.com/CHANGED.mp3", "track-f2"],
        )?;
        // This INSERT will fail — table doesn't exist.
        tx.execute(
            "INSERT INTO events (event_id, event_type, payload, subject_guid, signed_by, signature, created_at) \
             VALUES ('evt-track-f2', 'track.upserted', '{}', 'track-f2', 'pk', 'sig', ?1)",
            params![now],
        )?;
        tx.commit()
    })();

    // Restore the events table.
    conn.execute_batch("ALTER TABLE events_backup RENAME TO events")
        .expect("restore events table");

    assert!(
        result.is_err(),
        "transaction must fail when events table is missing"
    );

    // The enclosure_url must be UNCHANGED (UPDATE was rolled back).
    let url_after: String = conn
        .query_row(
            "SELECT enclosure_url FROM tracks WHERE track_guid = 'track-f2'",
            [],
            |r| r.get(0),
        )
        .expect("query");
    assert_eq!(
        url_after, "https://cdn.example.com/original.mp3",
        "enclosure_url must be unchanged when event insert failed — UPDATE must be rolled back"
    );
}
