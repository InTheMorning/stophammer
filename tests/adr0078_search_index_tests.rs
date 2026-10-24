// ADR 0078 task 001: the search index holds only searchable text.
//
// §1: `search_index` is a normal FTS5 table with the columns name, title and
// description. §2: a delete by rowid removes the old words. §6: one
// repair replaces an old table and fills the new table at the first open.

mod common;

use std::path::Path;

use rusqlite::{Connection, params};
use stophammer::db;
use stophammer::model::{Feed, Track};
use stophammer::search;

const OLD_DEFINITION: &str = "CREATE VIRTUAL TABLE search_index USING fts5( \
    entity_type, entity_id, name, title, description, tags, \
    content='', tokenize='unicode61')";

fn feed(guid: &str, title: &str, artist: &str, description: &str, medium: &str) -> Feed {
    Feed {
        feed_guid: guid.into(),
        feed_url: format!("https://example.com/{guid}.xml"),
        title: title.into(),
        title_lower: title.to_lowercase(),
        description: Some(description.into()),
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        itunes_type: None,
        release_artist: Some(artist.into()),
        release_artist_sort: None,
        release_date: None,
        release_kind: None,
        episode_count: 0,
        newest_item_at: None,
        oldest_item_at: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
        raw_medium: Some(medium.into()),
        last_build_date: None,
        release_artist_source: None,
    }
}

fn track(feed_guid: &str, guid: &str, title: &str) -> Track {
    Track {
        track_guid: guid.into(),
        feed_guid: feed_guid.into(),
        title: title.into(),
        title_lower: title.to_lowercase(),
        pub_date: None,
        duration_secs: None,
        enclosure_url: None,
        enclosure_type: None,
        enclosure_bytes: None,
        track_number: None,
        season: None,
        image_url: None,
        publisher: None,
        language: None,
        explicit: false,
        description: Some(String::new()),
        track_artist: None,
        track_artist_sort: None,
        created_at: 1_000_000,
        updated_at: 1_000_000,
    }
}

/// Writes one feed with one track and syncs the read models of the feed.
fn put_album(conn: &Connection, guid: &str, title: &str, track_title: &str) {
    db::upsert_feed(
        conn,
        &feed(guid, title, "Some Artist", "plain text", "music"),
    )
    .expect("write feed");
    db::upsert_track(conn, &track(guid, &format!("{guid}-t1"), track_title)).expect("write track");
    db::sync_source_read_models_for_feed(conn, guid).expect("sync read models");
}

fn hits(conn: &Connection, query: &str) -> Vec<(String, String)> {
    search::search(conn, query, None, 50, None, None)
        .expect("search")
        .into_iter()
        .map(|hit| (hit.entity_type, hit.entity_id))
        .collect()
}

fn index_definition(conn: &Connection) -> String {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE name = 'search_index'",
        [],
        |row| row.get(0),
    )
    .expect("read the definition of search_index")
}

fn entity_rows(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM search_entities", [], |row| row.get(0))
        .expect("count search_entities")
}

/// Turns the index of a database at `path` into the table that existed before
/// ADR 0078 §1, and keeps the rows of `search_entities` as they are.
fn use_old_definition(path: &Path) {
    let conn = Connection::open(path).expect("open raw connection");
    conn.execute_batch(&format!("DROP TABLE search_index; {OLD_DEFINITION};"))
        .expect("create the old table");
}

// Check 1.
#[test]
fn the_definition_has_three_text_columns_and_keeps_its_text() {
    let conn = common::test_db();
    let sql = index_definition(&conn);
    for needle in ["name", "title", "description"] {
        assert!(
            sql.contains(needle),
            "ADR 0078 §1: the definition must contain {needle}: {sql}"
        );
    }
    for needle in [
        "content=''",
        "contentless_delete",
        "entity_type",
        "entity_id",
        "tags",
    ] {
        assert!(
            !sql.contains(needle),
            "ADR 0078 §1: the definition must not contain {needle}: {sql}"
        );
    }
}

// The crate creates the table of §1.
#[test]
fn the_crate_creates_the_section_one_table() {
    let conn = Connection::open_in_memory().expect("open memory db");
    conn.execute_batch(
        "CREATE VIRTUAL TABLE t USING fts5(name, title, description, tokenize='unicode61'); \
         INSERT INTO t(rowid, name, title, description) VALUES (1, 'a', 'b', 'c'); \
         DELETE FROM t WHERE rowid = 1;",
    )
    .expect("ADR 0078 §1: the bundled SQLite must create the table");
}

// Check 2.
#[test]
fn a_changed_title_drops_the_old_words_for_a_feed_and_a_track() {
    let conn = common::test_db();
    put_album(&conn, "f-change", "Zebra Harvest", "Walrus Anthem");
    assert_eq!(
        hits(&conn, "zebra").len(),
        1,
        "the old feed title must match before the change"
    );

    put_album(&conn, "f-change", "Quokka Parade", "Narwhal Anthem");

    assert!(
        hits(&conn, "zebra").is_empty(),
        "ADR 0078 §2: the old feed title word must give no hit"
    );
    assert!(
        hits(&conn, "walrus").is_empty(),
        "ADR 0078 §2: the old track title word must give no hit"
    );
    let feed_hits = hits(&conn, "quokka");
    assert_eq!(
        feed_hits,
        vec![("feed".to_string(), "f-change".to_string())]
    );
    let track_hits = hits(&conn, "narwhal");
    assert_eq!(
        track_hits.len(),
        1,
        "the new track title must give the track one time"
    );
    assert_eq!(track_hits[0].0, "track", "the hit must be the track");
}

// Check 3.
#[test]
fn an_update_leaves_the_rank_of_an_unchanged_entity_as_in_a_new_database() {
    let changed = common::test_db();
    put_album(&changed, "f-keep", "Kestrel Moon", "Kestrel Song");
    put_album(&changed, "f-edit", "Kestrel Dawn", "Plain One");
    put_album(&changed, "f-edit", "Heron Dawn", "Plain Two");

    let fresh = common::test_db();
    put_album(&fresh, "f-keep", "Kestrel Moon", "Kestrel Song");
    put_album(&fresh, "f-edit", "Heron Dawn", "Plain Two");

    let rank = |conn: &Connection| {
        search::search(conn, "kestrel", Some("feed"), 10, None, None)
            .expect("search")
            .into_iter()
            .find(|hit| hit.entity_id == "f-keep")
            .expect("the unchanged feed must match")
            .rank
    };
    let (a, b) = (rank(&changed), rank(&fresh));
    assert!(
        (a - b).abs() < 1e-12,
        "ADR 0078 §2: the bm25 rank must equal the rank in a new database: {a} against {b}"
    );
}

// Check 7.
#[test]
fn internal_words_match_no_entity() {
    let conn = common::test_db();
    put_album(&conn, "f-words", "Quokka Parade", "Narwhal Anthem");
    for word in ["track", "feed", "music"] {
        assert!(
            hits(&conn, word).is_empty(),
            "ADR 0078 §1: q={word} must give no hit for an entity without the word"
        );
    }
}

// Check 10.
#[test]
fn an_old_definition_is_replaced_and_filled_one_time() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("old.db");
    {
        let conn = db::open_db(&path);
        put_album(&conn, "f-one", "Quokka Parade", "Narwhal Anthem");
        put_album(&conn, "f-two", "Heron Dawn", "Plain Two");
    }
    use_old_definition(&path);

    let conn = db::open_db(&path);
    assert!(
        !index_definition(&conn).contains("content=''"),
        "ADR 0078 §6: the open must replace the old definition"
    );
    // Two feeds and two tracks.
    assert_eq!(
        entity_rows(&conn),
        4,
        "ADR 0078 §6: one search_entities row for each entity of the source tables"
    );
    assert_eq!(
        hits(&conn, "quokka").len(),
        1,
        "the fill must be searchable"
    );

    // A sentinel row shows a second repair: the repair deletes each row of
    // search_entities and fills it from the source tables again.
    conn.execute(
        "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (1, 'sentinel', 's')",
        [],
    )
    .expect("write sentinel");
    drop(conn);
    let conn = db::open_db(&path);
    let sentinel: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = 'sentinel'",
            [],
            |row| row.get(0),
        )
        .expect("count sentinel");
    assert_eq!(sentinel, 1, "ADR 0078 §6: a second open must run no repair");
}

// Check 11.
#[test]
fn an_old_definition_with_a_bare_guid_track_row_opens_with_no_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("bare.db");
    {
        let conn = db::open_db(&path);
        put_album(&conn, "f-bare", "Quokka Parade", "Narwhal Anthem");
    }
    use_old_definition(&path);
    {
        let conn = Connection::open(&path).expect("open raw connection");
        let rowid = search::rowid_for("track", "bare-track-guid");
        conn.execute(
            "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (?1, 'track', 'bare-track-guid')",
            params![rowid],
        )
        .expect("write bare-GUID entity row");
        conn.execute(
            "INSERT INTO search_index(rowid, entity_type, entity_id, name, title, description, tags) \
             VALUES (?1, 'track', 'bare-track-guid', '', 'Narwhal Anthem', '', '')",
            params![rowid],
        )
        .expect("write bare-GUID index row");
    }

    let conn = db::try_open_db(&path).expect("ADR 0078 §6: the open must not fail");
    let bare: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities \
             WHERE entity_type = 'track' AND entity_id = 'bare-track-guid'",
            [],
            |row| row.get(0),
        )
        .expect("count bare rows");
    assert_eq!(bare, 0, "the bare-GUID row must be gone");
    assert_eq!(
        hits(&conn, "narwhal").len(),
        1,
        "the track must match one time"
    );
}

// Check 12.
#[test]
fn an_old_definition_with_a_stale_row_of_a_deleted_feed_opens_with_no_row() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("stale.db");
    {
        let conn = db::open_db(&path);
        put_album(&conn, "f-live", "Heron Dawn", "Plain Two");
    }
    use_old_definition(&path);
    {
        let conn = Connection::open(&path).expect("open raw connection");
        let rowid = search::rowid_for("feed", "f-gone");
        conn.execute(
            "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (?1, 'feed', 'f-gone')",
            params![rowid],
        )
        .expect("write stale entity row");
        conn.execute(
            "INSERT INTO search_index(rowid, entity_type, entity_id, name, title, description, tags) \
             VALUES (?1, 'feed', 'f-gone', '', 'Gone Feed', '', '')",
            params![rowid],
        )
        .expect("write stale index row");
    }

    let conn = db::open_db(&path);
    let stale: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_id = 'f-gone'",
            [],
            |row| row.get(0),
        )
        .expect("count stale rows");
    assert_eq!(stale, 0, "ADR 0078 §6: the stale row must be gone");
    assert!(
        hits(&conn, "gone").is_empty(),
        "the stale feed must not match"
    );
}
