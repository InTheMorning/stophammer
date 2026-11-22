# ADR 0106 Task 002: A Redirect With A GUID Change, And The Record At The Source URL

Owner: [ADR 0106](../adr/0106-a-feed-changes-its-guid-in-place.md) §6, §7,
§8 and the invariant on the source URL.

Repository: `stophammer`. `stophammer-crawler/` is a separate repository. Do
not change it.

Do this task after [task 001](adr-0106-task-001-guid-change-in-place.md).
Task 001 adds `GuidChangeWrite` and the GUID change in `ingest_transaction`.

## Goal

- `classify_submission` gives a GUID change only for the record whose
  `feed_url` is the `source_url` of the submission.
- The source URL `S` of record `R` answers `301` or `308` to `F`, and the body
  at `F` declares a free new GUID `G2`. One transaction changes `R` to `G2`
  and moves `R` to `F`.
- A declared new URL `F` (`itunes:new-feed-url` or self link) with a free
  `G2` at `F` stays a new feed. This is the current behavior. The task adds
  the test.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Submission classification | extend | `src/db.rs` | `classify_submission` reads one row for `feed_url IN (source_url, canonical_url)`. It must select the row by the source URL |
| Move decision | extend | `src/api.rs` | `move_target` gives the permanent redirect move for `Update`. The same rule applies to the record of a `GuidChange` |
| Ingest transaction | reuse | `src/db.rs` | `ingest_transaction` takes `feed_move` and, after task 001, `guid_change` in one transaction |
| Classification tests | extend | `tests/adr0051_classify_tests.rs` | `insert_feed`, `seed_two_feeds` and the constants `GA`, `GN`, `UA`, `UB`, `UX` |
| Redirect test helpers | reuse | `tests/adr0052_move_trigger_tests.rs` | `redirect_hop` and the ingest helpers |
| GUID change test file | extend | `tests/adr0106_guid_change_in_place_tests.rs` | Task 001 makes this file and its helpers |

## Files To Inspect

- `docs/adr/0106-a-feed-changes-its-guid-in-place.md`: §1, §6, §7, the
  invariants and acceptance criteria 8 to 10.
- `docs/adr/0052-a-source-moves-its-own-feed.md` §1, and
  `docs/adr/0083-*.md` §5 and §6.
- `src/db.rs`: `classify_submission`, `SubmissionClass`, `ingest_transaction`,
  `FeedMoveWrite`, `GuidChangeWrite`.
- `src/api.rs`: `move_target`, `MoveDecision`, the read phase of
  `handle_ingest_feed` that sets `req.force_reingest`, the match on the
  classification in the write phase (the `GuidChange` arm), and the code that
  makes `db::FeedMoveWrite` from the move target.
- `tests/adr0051_classify_tests.rs`, `tests/adr0052_move_trigger_tests.rs`,
  `tests/adr0083_move_refusal_tests.rs`,
  `tests/adr0106_guid_change_in_place_tests.rs`.

## Files Likely To Change

- `src/db.rs`, `src/api.rs`.
- `tests/adr0051_classify_tests.rs`,
  `tests/adr0106_guid_change_in_place_tests.rs`.

## Do Not Touch

- `docs/adr/`, `AGENTS.md`, `docs/plans/`, `docs/tasks/`.
- `stophammer-crawler/`.
- Git: do not commit, do not stage, and do not run a Git command that writes.

## Constraints

### The classification

`classify_submission(conn, feed_guid, source_url, canonical_url)`:

1. Read each record whose `feed_url` is `source_url` or `canonical_url` and
   whose `feed_guid` is not `feed_guid`. Do not read only one row.
2. No such record: keep the current result (`Update`, `Mirror` or `NewFeed`).
3. `feed_guid` is held by a record: `RecordConflict`. This is the current
   rule.
4. `feed_guid` is free, and the only such record has `feed_url =
   source_url`: `GuidChange { held_guid }` with the GUID of that record.
5. Each other case is `RecordConflict`. Two examples:
   - `R` holds `S` and `R2` holds `F`.
   - No record holds `S`, and `R2` holds `F`. A different URL cannot change
     the GUID of `R2`.

Keep the order of the checks. Each current test of
`tests/adr0051_classify_tests.rs` must pass without a change. One exception
is a test that expects `GuidChange` for a record held only at
`canonical_url`. If such a test
exists, change it to `RecordConflict` and report it.

### The permanent redirect

For `GuidChange { held_guid }`, `move_target` applies the permanent redirect
rule of the `Update` arm to the record `held_guid`:

- `source_url` is the stored `feed_url` of `held_guid`. Step 4 above makes
  this true.
- `redirects` is not empty, each hop is `301` or `308`, and `canonical_url`
  is not the stored `feed_url`.
- The ADR 0083 §6 rules on a left URL stay. They read the move state and the
  left URLs of `held_guid`. During a wait the answer is `WaitingRedirect`:
  the GUID change applies, and `R` keeps `S`.
- The `new-feed-url` and self link triggers do not apply to `GuidChange`.

Put the shared rule in one place. Do not copy the `Update` arm. For example,
move the redirect part into a helper that takes the GUID of the record, and
call it from both arms.

The write phase gives both `guid_change_write` and the `feed_move` to one call
of `ingest_transaction`. The `FeedMoveWrite` names the new GUID if
`ingest_transaction` applies the GUID change before the move. Read the order
in `ingest_transaction` and make the move use the GUID that the record has at
that step. The answer and the events give the move and the GUID change. A
`WaitingRedirect` gives the warning of ADR 0083 §7, as for `Update`.

The read phase sets `req.force_reingest` for a GUID change already. Keep it.

### The guards of a new GUID (ADR 0106 §8)

The `GuidChange` arm of the write phase does these checks before it sets
`guid_change_write`:

1. The ADR 0079 hold check of the `NewFeed` arm, on the declared GUID. Move
   the check into one helper that the `NewFeed` arm and the `GuidChange` arm
   both call. Do not copy it. The answer is `retired_source_conflict`.
2. The ADR 0057 block check. A body that blocks this index retires the record
   `held_guid` by the path of the `Update` arm, with `held_guid` in place of
   the declared GUID. The GUID does not change first. Use one helper for the
   `Update` arm and the `GuidChange` arm. Do not copy the retirement code.

### Old guard

ADR 0106 §6 deletes the ADR 0052 guard "A `301` from the source URL to a URL
that declares a different GUID moves nothing." Search the tests for a test
that asserts that guard, with any name. If one exists, change it to criterion
1 below or delete it, and report it.

## Acceptance Criteria (mechanical)

Each criterion is a test.

1. ADR 0106 criterion 8: `R` holds `S` with `G1`. `S` answers `301` to `F`,
   and `F` declares a free `G2`. After the ingest, one record has `G2` and the
   `feed_url` `F`. No record has `G1`. `GET /v1/feeds/{G1}` gives `404` with
   `superseded_by` `G2`. `F` is not held by a different record. The same test
   with `308` passes.
2. ADR 0106 criterion 9: `R` holds `S` with `G1`, `R2` holds `F` with `G3`.
   `S` answers `301` to `F`, and `F` declares a free `G2`. The answer has
   `accepted: false` and `reason: "record_conflict"`. Both records keep their
   GUIDs and URLs.
3. ADR 0106 criterion 10: `R` holds `S` with `G1` and declares
   `itunes:new-feed-url` `F`. A submission with `source_url` and
   `canonical_url` `F` declares a free `G2`. A new record holds `F` with
   `G2`. `R` keeps `G1` and `S`.
4. `classify_submission` unit tests in `tests/adr0051_classify_tests.rs`:
   - `R` at `S`, `R2` at `F`, free GUID, source `S`, canonical `F`:
     `RecordConflict`.
   - No record at `S`, `R2` at `F`, free GUID, source `S`, canonical `F`:
     `RecordConflict`.
   - `R` at `S`, no record at `F`, free GUID, source `S`, canonical `F`:
     `GuidChange` with the GUID of `R`.
5. During an ADR 0083 §6 wait for a redirect to a left URL `F`, a free `G2`
   at `F` changes `R` to `G2`, and `R` keeps `S`.
6. A `302` from `S` to `F` with a free `G2` at `F` changes `R` to `G2`, and
   `R` keeps `S`.
7. ADR 0106 criterion 11: `R` at `S` with `G1` declares `G2`. An active ADR
   0079 hold on `G2` names a different URL. The answer is
   `retired_source_conflict`, and `R` keeps `G1`.
8. ADR 0106 criterion 12: the same, but the hold names `S`. `R` changes to
   `G2`, and `retired_source_holds` has no row for `G2`.
9. ADR 0106 criterion 13: `R` at `S` with `G1` declares `G2` and a
   `podcast:block` for this index. `R` is retired under `G1`, and no record
   has `G2`.

Visual: none.

## Checks

Run in the repository root:

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

Report each result. Report each test that you changed or deleted, and why.

Write each comment in Simplified Technical English
(`/home/citizen/.agents/skills/asd-ste100/SKILL.md`).

## Operator Choices

- 2026-11-22, review of `6509bc5` (F6): after a change from `G1` to `G2`,
  the GUID `G1` is free. A different URL that declares `G1` makes a new
  record, as in Podcast Index. A live record has priority over a link, so
  each write of a feed row deletes the link whose old GUID is the GUID of
  that row. The primary and each community node delete the same link. ADR
  0106 §3 gives the link only for an old GUID that no record holds, so this
  needs no new ADR.
