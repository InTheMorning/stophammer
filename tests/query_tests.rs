mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Insert a test feed. ADR 0034 §11: `feeds` carries no artist credit.
fn insert_feed(conn: &rusqlite::Connection, guid: &str, url: &str, title: &str) {
    let now = common::now();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, \
         explicit, episode_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)",
        params![guid, url, title, title.to_lowercase(), now],
    )
    .unwrap();
}

/// Insert a test track. ADR 0034 §11: `tracks` carries no artist credit.
fn insert_track(
    conn: &rusqlite::Connection,
    guid: &str,
    feed_guid: &str,
    title: &str,
    pub_date: Option<i64>,
) {
    let now = common::now();
    conn.execute(
        "INSERT INTO tracks (track_guid, feed_guid, title, title_lower, \
         pub_date, explicit, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)",
        params![guid, feed_guid, title, title.to_lowercase(), pub_date, now],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// ADR 0034 §11 — 2026-09-28: `artists`, `artist_aliases` and
// `artist_credit_name` are dropped, and a feed no longer joins to an artist
// through a credit. The tests that read those tables and that join
// (`get_artist_by_id`, `get_artist_not_found`, `get_artist_aliases`,
// `get_feeds_for_artist`) are removed with them.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 6. get_feeds_pagination — insert 5 feeds, paginate with limit=2 via cursor
//    Uses (title_lower, feed_guid) composite cursor for stable ordering.
// ---------------------------------------------------------------------------

#[test]
fn get_feeds_pagination() {
    let conn = common::test_db();

    // Insert 5 feeds with deterministic, sortable titles.
    for i in 1..=5_u32 {
        insert_feed(
            &conn,
            &format!("page-feed-{i:02}"),
            &format!("https://example.com/page{i}.xml"),
            &format!("Page Feed {i:02}"),
        );
    }

    // Page 1: no cursor — fetch first 2.
    let page1: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT feed_guid, title_lower FROM feeds \
                 ORDER BY title_lower ASC, feed_guid ASC LIMIT ?1",
            )
            .unwrap();
        stmt.query_map(params![2], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(page1.len(), 2);
    assert_eq!(page1[0].0, "page-feed-01");
    assert_eq!(page1[1].0, "page-feed-02");

    // Build composite cursor from last item on page 1: title_lower:feed_guid
    let cursor_title = &page1.last().unwrap().1;
    let cursor_guid = &page1.last().unwrap().0;

    // Page 2.
    let page2: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT feed_guid, title_lower FROM feeds \
                 WHERE (title_lower > ?1 OR (title_lower = ?1 AND feed_guid > ?2)) \
                 ORDER BY title_lower ASC, feed_guid ASC LIMIT ?3",
            )
            .unwrap();
        stmt.query_map(params![cursor_title, cursor_guid, 2], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    assert_eq!(page2.len(), 2);
    assert_eq!(page2[0].0, "page-feed-03");
    assert_eq!(page2[1].0, "page-feed-04");

    // Page 3: only one item remaining.
    let cursor_title2 = &page2.last().unwrap().1;
    let cursor_guid2 = &page2.last().unwrap().0;

    let page3: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT feed_guid, title_lower FROM feeds \
                 WHERE (title_lower > ?1 OR (title_lower = ?1 AND feed_guid > ?2)) \
                 ORDER BY title_lower ASC, feed_guid ASC LIMIT ?3",
            )
            .unwrap();
        stmt.query_map(params![cursor_title2, cursor_guid2, 2], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    };
    assert_eq!(page3.len(), 1);
    assert_eq!(page3[0].0, "page-feed-05");
}

// ---------------------------------------------------------------------------
// 6b. cursor_stability_duplicate_titles — 3 feeds with same title_lower,
//     paginate with limit=2, verify no skips or duplicates.
//     Issue-CURSOR-STABILITY — 2026-03-14
// ---------------------------------------------------------------------------

#[test]
fn cursor_stability_duplicate_titles() {
    let conn = common::test_db();

    // All three feeds share the same title_lower = "ep".
    // feed_guids are chosen so their sort order is deterministic: aaa < bbb < ccc
    insert_feed(&conn, "aaa-guid", "https://example.com/aaa.xml", "EP");
    insert_feed(&conn, "bbb-guid", "https://example.com/bbb.xml", "EP");
    insert_feed(&conn, "ccc-guid", "https://example.com/ccc.xml", "EP");

    // Page 1: fetch first 2, ordered by (title_lower, feed_guid).
    let page1: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT feed_guid FROM feeds \
                 ORDER BY title_lower ASC, feed_guid ASC \
                 LIMIT ?1",
            )
            .unwrap();
        stmt.query_map(params![2], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(page1.len(), 2, "page 1 should have 2 feeds");
    assert_eq!(page1[0], "aaa-guid");
    assert_eq!(page1[1], "bbb-guid");

    // Build composite cursor from last item: title_lower:feed_guid
    let last_guid = page1.last().unwrap();
    let cursor_title: String = conn
        .query_row(
            "SELECT title_lower FROM feeds WHERE feed_guid = ?1",
            params![last_guid],
            |r| r.get(0),
        )
        .unwrap();

    // Page 2: use composite cursor to fetch next page.
    let page2: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT feed_guid FROM feeds \
                 WHERE (title_lower > ?1 OR (title_lower = ?1 AND feed_guid > ?2)) \
                 ORDER BY title_lower ASC, feed_guid ASC \
                 LIMIT ?3",
            )
            .unwrap();
        stmt.query_map(params![cursor_title, last_guid, 2], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(page2.len(), 1, "page 2 should have the remaining 1 feed");
    assert_eq!(page2[0], "ccc-guid");

    // Verify no duplicates across all pages.
    let mut all_guids = page1;
    all_guids.extend(page2);
    all_guids.sort();
    all_guids.dedup();
    assert_eq!(
        all_guids.len(),
        3,
        "exactly 3 unique feeds across both pages"
    );
}

// ---------------------------------------------------------------------------
// 7. get_feed_by_guid — insert feed, query by guid, verify fields
// ---------------------------------------------------------------------------

#[test]
fn get_feed_by_guid() {
    let conn = common::test_db();

    let guid = "portishead-dummy";
    let url = "https://example.com/portishead.xml";
    let title = "Dummy";

    insert_feed(&conn, guid, url, title);

    let (returned_guid, returned_url, returned_title): (String, String, String) = conn
        .query_row(
            "SELECT feed_guid, feed_url, title \
             FROM feeds WHERE feed_guid = ?1",
            params![guid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();

    assert_eq!(returned_guid, guid);
    assert_eq!(returned_url, url);
    assert_eq!(returned_title, title);
}

// ---------------------------------------------------------------------------
// 8. get_feed_with_tracks — insert feed + 3 tracks, query tracks, verify count
// ---------------------------------------------------------------------------

#[test]
fn get_feed_with_tracks() {
    let conn = common::test_db();
    let base_date: i64 = 1_700_000_000;

    let feed_guid = "mezzanine-feed";
    insert_feed(
        &conn,
        feed_guid,
        "https://example.com/mezzanine.xml",
        "Mezzanine",
    );

    // Insert 3 tracks with distinct pub_dates for ordering.
    let tracks = [
        ("track-angel", "Angel", base_date + 300),
        ("track-risingson", "Risingson", base_date + 200),
        ("track-teardrop", "Teardrop", base_date + 100),
    ];
    for (guid, title, pub_date) in &tracks {
        insert_track(&conn, guid, feed_guid, title, Some(*pub_date));
    }

    // Query all tracks for the feed, ordered by pub_date DESC.
    let mut stmt = conn
        .prepare(
            "SELECT track_guid, title, pub_date \
             FROM tracks WHERE feed_guid = ?1 ORDER BY pub_date DESC",
        )
        .unwrap();

    let rows: Vec<(String, String, Option<i64>)> = stmt
        .query_map(params![feed_guid], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(rows.len(), 3, "should return all 3 tracks for the feed");

    // Ordered by pub_date DESC: Angel (300) → Risingson (200) → Teardrop (100).
    assert_eq!(rows[0].0, "track-angel");
    assert_eq!(rows[0].1, "Angel");
    assert_eq!(rows[1].0, "track-risingson");
    assert_eq!(rows[1].1, "Risingson");
    assert_eq!(rows[2].0, "track-teardrop");
    assert_eq!(rows[2].1, "Teardrop");
}
