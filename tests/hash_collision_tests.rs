// Issue-HASH-COLLISION — 2026-03-14
// Tests for FTS5 rowid hash collision detection in populate_search_index.

mod common;

use rusqlite::params;
use stophammer::search;

// ---------------------------------------------------------------------------
// 1. collision_detected_skips_second_entity — pre-occupy the rowid that
//    the colliding entity would compute with a different (incumbent) entity,
//    then call populate_search_index for the collider and verify it is
//    gracefully skipped.
// ---------------------------------------------------------------------------

#[test]
fn collision_detected_skips_second_entity() {
    let conn = common::test_db();

    // The colliding entity whose rowid we will pre-occupy.
    let collider_type = "track";
    let collider_id   = "track-bbb";
    let target_rowid  = search::rowid_for(collider_type, collider_id);

    // Pre-occupy target_rowid with an incumbent entity (different identity),
    // simulating the state after a prior insert that happened to produce the
    // same hash.
    let incumbent_type = "feed";
    let incumbent_id   = "feed-aaa";

    conn.execute(
        "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (?1, ?2, ?3)",
        params![target_rowid, incumbent_type, incumbent_id],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO search_index(rowid, entity_type, entity_id, name, title, description, tags) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![target_rowid, incumbent_type, incumbent_id, "Alpha Feed", "Alpha Title", "desc-a", "rock"],
    )
    .unwrap();

    // Now call populate_search_index for the collider — its computed rowid is
    // target_rowid, which is occupied by the incumbent.  This should be
    // detected as a collision and return Ok(()) without error.
    let result = search::populate_search_index(
        &conn, collider_type, collider_id, "Beta Track", "Beta Title", "desc-b", "jazz",
    );
    assert!(result.is_ok(), "collision should not produce an error");

    // The search_entities row at target_rowid must still belong to the incumbent.
    let (etype, eid): (String, String) = conn
        .query_row(
            "SELECT entity_type, entity_id FROM search_entities WHERE rowid = ?1",
            params![target_rowid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(etype, incumbent_type, "collision must not overwrite entity_type");
    assert_eq!(eid, incumbent_id, "collision must not overwrite entity_id");

    // The collider must NOT appear in search_entities at all.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_entities WHERE entity_type = ?1 AND entity_id = ?2",
            params![collider_type, collider_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "colliding entity must not appear in search_entities");
}

// ---------------------------------------------------------------------------
// 2. same_entity_idempotent — calling populate_search_index twice for the
//    exact same entity must succeed (update path, not a collision).
// ---------------------------------------------------------------------------

#[test]
fn same_entity_idempotent() {
    let conn = common::test_db();

    search::populate_search_index(
        &conn, "feed", "f1", "Original Name", "Original Title", "", "",
    )
    .unwrap();

    // Second call with updated text — must succeed, not be treated as collision.
    search::populate_search_index(
        &conn, "feed", "f1", "Updated Name", "Updated Title", "", "",
    )
    .unwrap();

    // The companion table must still map to the same entity.
    let rowid = search::rowid_for("feed", "f1");
    let (etype, eid): (String, String) = conn
        .query_row(
            "SELECT entity_type, entity_id FROM search_entities WHERE rowid = ?1",
            params![rowid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(etype, "feed");
    assert_eq!(eid, "f1");

    // Verify search finds the updated text.
    let results = search::search(&conn, "Updated", None, 10, None, None).unwrap();
    assert_eq!(results.len(), 1, "updated entity should be searchable");
    assert_eq!(results[0].entity_id, "f1");
}

// ---------------------------------------------------------------------------
// 3. collision_preserves_first_entity_search — the incumbent entity remains
//    findable via FTS5 search after a collision occurs.
// ---------------------------------------------------------------------------

#[test]
fn collision_preserves_first_entity_search() {
    let conn = common::test_db();

    // The colliding entity whose rowid we will pre-occupy.
    let collider_type = "track";
    let collider_id   = "track-beta";
    let target_rowid  = search::rowid_for(collider_type, collider_id);

    // Pre-occupy target_rowid with an incumbent entity.
    let incumbent_type = "feed";
    let incumbent_id   = "feed-alpha";

    conn.execute(
        "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (?1, ?2, ?3)",
        params![target_rowid, incumbent_type, incumbent_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO search_index(rowid, entity_type, entity_id, name, title, description, tags) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![target_rowid, incumbent_type, incumbent_id, "UniqueAlphaName", "", "", ""],
    )
    .unwrap();

    // Trigger collision for the collider.
    let _ = search::populate_search_index(
        &conn, collider_type, collider_id, "BetaTrack", "", "", "",
    );

    // The incumbent must still be searchable by its name.
    let results = search::search(&conn, "UniqueAlphaName", None, 10, None, None).unwrap();
    assert!(
        results.iter().any(|r| r.entity_id == incumbent_id),
        "incumbent entity must remain in search results after collision",
    );
}

// ---------------------------------------------------------------------------
// 4. unique_constraint_prevents_duplicate_entity — the UNIQUE index on
//    (entity_type, entity_id) rejects a raw INSERT of the same entity at a
//    different rowid.
// ---------------------------------------------------------------------------

#[test]
fn unique_constraint_prevents_duplicate_entity() {
    let conn = common::test_db();

    conn.execute(
        "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (?1, ?2, ?3)",
        params![100, "feed", "f-dup"],
    )
    .unwrap();

    // Attempt to insert the same (entity_type, entity_id) at a different rowid.
    let result = conn.execute(
        "INSERT INTO search_entities (rowid, entity_type, entity_id) VALUES (?1, ?2, ?3)",
        params![200, "feed", "f-dup"],
    );

    assert!(
        result.is_err(),
        "UNIQUE constraint on (entity_type, entity_id) should reject duplicate",
    );
}
