// Stable FTS5 hash tests.

use std::collections::HashSet;

use stophammer::search::rowid_for;

// ── TC-06a: deterministic across calls ──────────────────────────────────────

#[test]
fn test_rowid_deterministic_across_calls() {
    let a = rowid_for("feed", "some-guid-abc");
    let b = rowid_for("feed", "some-guid-abc");
    assert_eq!(
        a, b,
        "rowid_for must return the same value for identical inputs"
    );
}

// ── TC-06b: known vector (stability test) ───────────────────────────────────

#[test]
fn test_rowid_known_vector() {
    // This value was computed with SipHash-2-4(key=0,0) over "feed\0test-guid-123".
    // If this test fails, the hash algorithm changed and the FTS5 index needs
    // rebuilding.
    let expected: i64 = 1_076_195_953_416_371_681;
    assert_eq!(
        rowid_for("feed", "test-guid-123"),
        expected,
        "known-vector mismatch: the FTS5 hash function has changed"
    );
}

// ── TC-06c: no collisions for 10k pairs ─────────────────────────────────────

#[test]
fn test_rowid_no_collision_for_10k_pairs() {
    let mut seen = HashSet::with_capacity(10_000);
    for i in 0..10_000 {
        let entity_type = if i % 3 == 0 {
            "feed"
        } else if i % 3 == 1 {
            "track"
        } else {
            "artist"
        };
        let entity_id = format!("entity-{i}");
        let rid = rowid_for(entity_type, &entity_id);
        assert!(
            seen.insert(rid),
            "collision at i={i}: rowid {rid} already seen"
        );
    }
}

// ── TC-06d: swapped inputs produce different rowids ─────────────────────────

#[test]
fn test_rowid_different_for_swapped_inputs() {
    let a = rowid_for("feed", "abc");
    let b = rowid_for("abc", "feed");
    assert_ne!(
        a, b,
        "rowid_for(\"feed\", \"abc\") must differ from rowid_for(\"abc\", \"feed\")"
    );
}
