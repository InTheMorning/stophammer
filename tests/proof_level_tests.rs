// Issue-PROOF-LEVEL — 2026-03-14

mod common;

use stophammer::proof::ProofLevel;

// ---------------------------------------------------------------------------
// ProofLevel serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn proof_level_serializes_to_expected_strings() {
    let rss_only = serde_json::to_value(ProofLevel::RssOnly).expect("serialize RssOnly");
    assert_eq!(rss_only, serde_json::json!("rss_only"));

    let rss_and_audio =
        serde_json::to_value(ProofLevel::RssAndAudio).expect("serialize RssAndAudio");
    assert_eq!(rss_and_audio, serde_json::json!("rss_and_audio"));

    let relocation =
        serde_json::to_value(ProofLevel::RelocationProven).expect("serialize RelocationProven");
    assert_eq!(relocation, serde_json::json!("relocation_proven"));
}

#[test]
fn proof_level_deserializes_from_strings() {
    let rss_only: ProofLevel = serde_json::from_str("\"rss_only\"").expect("deserialize rss_only");
    assert_eq!(rss_only, ProofLevel::RssOnly);

    let rss_and_audio: ProofLevel =
        serde_json::from_str("\"rss_and_audio\"").expect("deserialize rss_and_audio");
    assert_eq!(rss_and_audio, ProofLevel::RssAndAudio);

    let relocation: ProofLevel =
        serde_json::from_str("\"relocation_proven\"").expect("deserialize relocation_proven");
    assert_eq!(relocation, ProofLevel::RelocationProven);
}

// ---------------------------------------------------------------------------
// proof_tokens table has proof_level column after migrations
// ---------------------------------------------------------------------------

#[test]
fn proofs_table_has_proof_level_column() {
    let conn = common::test_db();

    let columns: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(proof_tokens)")
            .expect("prepare PRAGMA");
        stmt.query_map([], |row| row.get::<_, String>(1))
            .expect("query columns")
            .collect::<Result<_, _>>()
            .expect("collect column names")
    };

    assert!(
        columns.contains(&"proof_level".to_string()),
        "proof_tokens must contain a proof_level column, found: {columns:?}"
    );
}

#[test]
fn proof_level_column_defaults_to_rss_only() {
    let conn = common::test_db();
    let now = common::now();

    // Insert a token without specifying proof_level — the DEFAULT should apply.
    conn.execute(
        "INSERT INTO proof_tokens (access_token, scope, subject_feed_guid, expires_at, created_at) \
         VALUES ('tok_default', 'feed:write', 'guid-1', ?1, ?2)",
        rusqlite::params![now + 3600, now],
    )
    .expect("insert token without explicit proof_level");

    let level: String = conn
        .query_row(
            "SELECT proof_level FROM proof_tokens WHERE access_token = 'tok_default'",
            [],
            |r| r.get(0),
        )
        .expect("read proof_level");

    assert_eq!(level, "rss_only", "default proof_level must be 'rss_only'");
}

// ---------------------------------------------------------------------------
// issue_token sets proof_level = RssOnly
// ---------------------------------------------------------------------------

#[test]
fn issue_token_sets_rss_only_proof_level() {
    let conn = common::test_db();
    let now = common::now();

    // Seed a minimal feed so that issue_token's INSERT succeeds (no FK on
    // proof_tokens, but we want a realistic setup).
    conn.execute(
        "INSERT INTO artists (artist_id, name, name_lower, created_at, updated_at) \
         VALUES ('a1', 'Test', 'test', ?1, ?2)",
        rusqlite::params![now, now],
    )
    .expect("seed artist");
    conn.execute(
        "INSERT INTO artist_credit (display_name, created_at) VALUES ('Test', ?1)",
        rusqlite::params![now],
    )
    .expect("seed artist_credit");
    let credit_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO feeds (feed_guid, feed_url, title, title_lower, artist_credit_id, \
         description, explicit, episode_count, created_at, updated_at) \
         VALUES ('guid-1', 'https://example.com/feed.xml', 'T', 't', ?1, '', 0, 0, ?2, ?3)",
        rusqlite::params![credit_id, now, now],
    )
    .expect("seed feed");

    let token = stophammer::proof::issue_token(&conn, "feed:write", "guid-1", &ProofLevel::RssOnly)
        .expect("issue_token should succeed");

    let level: String = conn
        .query_row(
            "SELECT proof_level FROM proof_tokens WHERE access_token = ?1",
            rusqlite::params![token],
            |r| r.get(0),
        )
        .expect("read proof_level for issued token");

    assert_eq!(
        level, "rss_only",
        "issue_token must store proof_level as 'rss_only'"
    );
}
