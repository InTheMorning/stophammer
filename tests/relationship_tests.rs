mod common;

use rusqlite::params;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn insert_artist(conn: &rusqlite::Connection, name: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = common::now();
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, name.to_lowercase(), now, now],
    )
    .unwrap();
    id
}

// ---------------------------------------------------------------------------
// 1. artist_artist_rel_created
// ---------------------------------------------------------------------------

/// Create 2 artists, insert into `artist_artist_rel` with `rel_type_id`=20
/// (`member_of`), verify the row is stored with correct fields.
#[test]
fn artist_artist_rel_created() {
    let conn = common::test_db();
    let now = common::now();

    let artist_a = insert_artist(&conn, "Jack White");
    let artist_b = insert_artist(&conn, "The White Stripes");

    conn.execute(
        "INSERT INTO artist_artist_rel (artist_id_a, artist_id_b, rel_type_id, begin_year, end_year, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![artist_a, artist_b, 20, 1997, 2011, now],
    )
    .unwrap();

    let (stored_a, stored_b, stored_rel, stored_begin, stored_end): (
        String,
        String,
        i64,
        Option<i64>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT artist_id_a, artist_id_b, rel_type_id, begin_year, end_year \
             FROM artist_artist_rel WHERE artist_id_a = ?1 AND artist_id_b = ?2",
            params![artist_a, artist_b],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();

    assert_eq!(stored_a, artist_a);
    assert_eq!(stored_b, artist_b);
    assert_eq!(stored_rel, 20);
    assert_eq!(stored_begin, Some(1997));
    assert_eq!(stored_end, Some(2011));
}

// ---------------------------------------------------------------------------
// 2. rel_type_lookup
// ---------------------------------------------------------------------------

/// Query `rel_type` table for id=1 (performer), verify name and `entity_pair`.
#[test]
fn rel_type_lookup() {
    let conn = common::test_db();

    let (name, entity_pair): (String, String) = conn
        .query_row(
            "SELECT name, entity_pair FROM rel_type WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

    assert_eq!(name, "performer");
    assert_eq!(entity_pair, "artist-track");
}

// ---------------------------------------------------------------------------
// 3. artist_rels_bidirectional
// ---------------------------------------------------------------------------

/// Create 2 artists, add rel from A->B, query rels for artist B using
/// `WHERE artist_id_a = ?1 OR artist_id_b = ?1`, verify B can find the relationship.
#[test]
fn artist_rels_bidirectional() {
    let conn = common::test_db();
    let now = common::now();

    let artist_a = insert_artist(&conn, "John Lennon");
    let artist_b = insert_artist(&conn, "The Beatles");

    conn.execute(
        "INSERT INTO artist_artist_rel (artist_id_a, artist_id_b, rel_type_id, created_at) \
         VALUES (?1, ?2, ?3, ?4)",
        params![artist_a, artist_b, 20, now],
    )
    .unwrap();

    // Query from artist B's perspective — should find the relationship.
    let mut stmt = conn
        .prepare(
            "SELECT artist_id_a, artist_id_b, rel_type_id \
             FROM artist_artist_rel \
             WHERE artist_id_a = ?1 OR artist_id_b = ?1",
        )
        .unwrap();

    let rows: Vec<(String, String, i64)> = stmt
        .query_map(params![artist_b], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(
        rows.len(),
        1,
        "artist B should find exactly one relationship"
    );
    assert_eq!(rows[0].0, artist_a);
    assert_eq!(rows[0].1, artist_b);
    assert_eq!(rows[0].2, 20);
}
