//! ADR 0051 section 2: `classify_submission` gives the case of a submission
//! before any write.
//!
//! Feed A has GUID `GA` at URL `UA`. Feed B has GUID `GB` at URL `UB`. Every
//! test seeds both feeds, then classifies a submission that names neither
//! GUID's row directly, following the table in
//! `docs/tasks/adr-0051-task-002-classify-submission.md`.

mod common;

use rusqlite::{Connection, params};
use stophammer::db::{self, SubmissionClass};

const GA: &str = "feed-a-guid";
const UA: &str = "https://example.com/a.xml";
const GB: &str = "feed-b-guid";
const UB: &str = "https://example.com/b.xml";
const GN: &str = "feed-n-guid";
const UX: &str = "https://example.com/x.xml";

/// Inserts a feed row with a valid artist credit, matching the pattern in
/// `tests/db_tests.rs`.
fn insert_feed(conn: &Connection, feed_guid: &str, feed_url: &str) {
    let now = common::now();
    let artist = stophammer::model::Artist {
        artist_id: format!("artist-{feed_guid}"),
        name: format!("Artist {feed_guid}"),
        name_lower: format!("artist {feed_guid}"),
        sort_name: None,
        type_id: None,
        area: None,
        img_url: None,
        url: None,
        begin_year: None,
        end_year: None,
        created_at: now,
        updated_at: now,
    };
    db::upsert_artist_if_absent(conn, &artist).expect("upsert artist");
    let credit = db::get_or_create_artist_credit(
        conn,
        &artist.name,
        &[(artist.artist_id.clone(), artist.name.clone(), String::new())],
        Some(feed_guid),
    )
    .expect("artist credit");
    let title = format!("Feed {feed_guid}");
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![feed_guid, feed_url, title, title.to_lowercase(), credit.id, now],
    )
    .expect("insert feed");
}

/// Seeds feed A at `UA` and feed B at `UB`, per the task table.
fn seed_two_feeds(conn: &Connection) {
    insert_feed(conn, GA, UA);
    insert_feed(conn, GB, UB);
}

fn feeds_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM feeds", [], |row| row.get(0))
        .expect("count feeds")
}

#[test]
fn same_url_for_both_fields_is_update() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GA, UA, UA).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::Update,
        "own source URL in both fields is an update"
    );
}

#[test]
fn redirect_from_source_url_is_update() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GA, UA, UX).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::Update,
        "a redirect from the source URL in canonical_url is still an update"
    );
}

#[test]
fn redirect_to_source_url_is_update() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GA, UX, UA).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::Update,
        "a redirect to the source URL in source_url is still an update"
    );
}

#[test]
fn neither_field_matches_own_record_is_mirror() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GA, UX, UX).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::Mirror {
            source_url: UA.to_string()
        },
        "neither field names the held record's own source URL, so this is a mirror"
    );
}

#[test]
fn both_fields_at_the_other_records_url_is_record_conflict() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GA, UB, UB).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::RecordConflict,
        "GA is held and UB belongs to GB, so this touches two held records"
    );
}

#[test]
fn split_fields_naming_the_other_record_is_record_conflict() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GA, UA, UB).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::RecordConflict,
        "GA is held and canonical_url names GB's URL, so this touches two held records"
    );
}

#[test]
fn unheld_guid_at_a_held_url_is_guid_change() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GN, UA, UA).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::GuidChange {
            held_guid: GA.to_string()
        },
        "GN is not held, and UA already belongs to GA"
    );
}

#[test]
fn unheld_guid_at_an_unheld_url_is_new_feed() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let result = db::classify_submission(&conn, GN, UX, UX).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::NewFeed,
        "neither the GUID nor the URL is held by any record"
    );
}

#[test]
fn trailing_slash_is_a_different_url_and_is_mirror() {
    let conn = common::test_db();
    seed_two_feeds(&conn);
    let ua_with_slash = format!("{UA}/");

    let result =
        db::classify_submission(&conn, GA, &ua_with_slash, &ua_with_slash).expect("classify");

    assert_eq!(
        result,
        SubmissionClass::Mirror {
            source_url: UA.to_string()
        },
        "the comparison is exact string equality, so a trailing slash names a different URL"
    );
}

#[test]
fn classify_submission_writes_no_row() {
    let conn = common::test_db();
    seed_two_feeds(&conn);

    let count_before = feeds_row_count(&conn);
    let target_row_before = db::get_feed(&conn, GA)
        .expect("get feed a")
        .expect("feed a exists");
    let bystander_row_before = db::get_feed(&conn, GB)
        .expect("get feed b")
        .expect("feed b exists");

    // One call per case, so a write hiding behind any branch would show up.
    db::classify_submission(&conn, GA, UA, UA).expect("classify update");
    db::classify_submission(&conn, GA, UX, UX).expect("classify mirror");
    db::classify_submission(&conn, GA, UB, UB).expect("classify record conflict");
    db::classify_submission(&conn, GN, UA, UA).expect("classify guid change");
    db::classify_submission(&conn, GN, UX, UX).expect("classify new feed");

    let count_after = feeds_row_count(&conn);
    let target_row_after = db::get_feed(&conn, GA)
        .expect("get feed a")
        .expect("feed a still exists");
    let bystander_row_after = db::get_feed(&conn, GB)
        .expect("get feed b")
        .expect("feed b still exists");

    assert_eq!(
        count_before, count_after,
        "classify_submission must not change the row count"
    );
    assert_eq!(
        target_row_before, target_row_after,
        "classify_submission must not change feed A's row"
    );
    assert_eq!(
        bystander_row_before, bystander_row_after,
        "classify_submission must not change feed B's row"
    );
}
