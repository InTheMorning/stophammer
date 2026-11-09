// ADR 0093 §4: migration 0064 clears each stored `content_link` that is
// an enclosure URL of the same live item, and keeps each other value.

use std::fs;
use std::path::PathBuf;

fn apply_migration_files_through(conn: &rusqlite::Connection, upper_file_name: &str) {
    let mut paths: Vec<PathBuf> = fs::read_dir("migrations")
        .expect("read migrations directory")
        .map(|entry| entry.expect("read migration entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    paths.sort();
    for path in paths {
        let file_name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("migration path should have UTF-8 file name");
        if file_name <= upper_file_name {
            let sql = fs::read_to_string(&path).expect("read migration SQL");
            conn.execute_batch(&sql).expect("apply migration SQL");
        }
    }
}

fn insert_live_event(conn: &rusqlite::Connection, guid: &str, content_link: Option<&str>) {
    conn.execute(
        "INSERT INTO live_events \
         (live_item_guid, feed_guid, title, content_link, status, created_at, updated_at) \
         VALUES (?1, 'live-feed', ?1, ?2, 'live', 1700000000, 1700000000)",
        rusqlite::params![guid, content_link],
    )
    .expect("insert live event");
}

fn insert_enclosure(conn: &rusqlite::Connection, guid: &str, position: i64, url: &str) {
    conn.execute(
        "INSERT INTO source_item_enclosures \
         (feed_guid, entity_type, entity_id, position, url, source, extraction_path, observed_at) \
         VALUES ('live-feed', 'live_item', ?1, ?2, ?3, 'rss_enclosure', 'live_item.enclosure', \
                 1700000000)",
        rusqlite::params![guid, position, url],
    )
    .expect("insert enclosure");
}

fn content_link(conn: &rusqlite::Connection, guid: &str) -> Option<String> {
    conn.query_row(
        "SELECT content_link FROM live_events WHERE live_item_guid = ?1",
        [guid],
        |r| r.get(0),
    )
    .expect("read content_link")
}

#[test]
fn migration_0064_clears_a_stream_and_keeps_a_content_link_page() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let conn = rusqlite::Connection::open(dir.path().join("adr0093.db")).expect("open db");
    apply_migration_files_through(&conn, "0063_title_sort_order_key.sql");
    // The rows below have no feed row; this test only examines the update.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("disable foreign keys for this fixture");

    let stream = "https://stream.example/live.m3u8";
    let alternate = "https://stream.example/live.mp3";

    // The enclosure fallback of the old parser.
    insert_live_event(&conn, "stream", Some(stream));
    insert_enclosure(&conn, "stream", 0, stream);
    // An alternate enclosure is also a stream.
    insert_live_event(&conn, "alternate", Some(alternate));
    insert_enclosure(&conn, "alternate", 0, stream);
    insert_enclosure(&conn, "alternate", 1, alternate);
    // A page that is not an enclosure of its live item stays.
    insert_live_event(&conn, "page", Some("https://page.example/live"));
    insert_enclosure(&conn, "page", 0, stream);
    // An enclosure of a different live item does not clear the value.
    insert_live_event(&conn, "other", Some(stream));
    insert_live_event(&conn, "none", None);

    let sql = fs::read_to_string("migrations/0064_live_content_link_not_stream.sql")
        .expect("read migration 0064");
    conn.execute_batch(&sql).expect("apply migration 0064");

    assert_eq!(
        content_link(&conn, "stream"),
        None,
        "ADR 0093 §4: a content_link equal to the enclosure of its live item is cleared"
    );
    assert_eq!(
        content_link(&conn, "alternate"),
        None,
        "ADR 0093 §4: a content_link equal to an alternate enclosure is cleared"
    );
    assert_eq!(
        content_link(&conn, "page").as_deref(),
        Some("https://page.example/live"),
        "ADR 0093 §4: a content_link that is not a stream stays"
    );
    assert_eq!(
        content_link(&conn, "other").as_deref(),
        Some(stream),
        "ADR 0093 §4: only an enclosure of the same live item clears the value"
    );
    assert_eq!(content_link(&conn, "none"), None);
}
