# Phase Plan: Compression, Value Time Splits And The Feed GUID

This plan is advisory. It gives the sequence of the tasks of three decisions:

- [ADR 0072](../adr/0072-a-feed-fetch-accepts-compression.md): a feed fetch
  accepts compression.
- [ADR 0073](../adr/0073-a-value-time-split-keeps-both-forms.md): a value time
  split keeps both forms.
- [ADR 0074](../adr/0074-a-feed-without-a-guid-is-invalid.md): a feed without a
  GUID is invalid.

Each ADR owns its rules. The three come from
[the research request](research-crawler-compression-and-value-time-split-request.md)
of 2026-10-20 and
[its verification](../reviews/research-crawler-compression-and-value-time-split-verification.md).

## Goal

- The crawler transfers compressed feed bodies and keeps its `304` answers.
- The index keeps each value time split that the specification permits.
- No mode accepts a feed body with no `podcast:guid`, and the skip records stay
  correct.

## Non-Goals

- ADR 0074 §4, the records that have no source GUID. It waits for the count of
  `.idea/guid/README.md`.
- The recipients of a split in the route history of ADR 0053.
- A parse warning channel in the parser.
- `zstd` and `deflate`.
- The archive move of ADR 0011. The orchestrator does it after acceptance.

## Assumptions

- The three ADRs are Accepted before the first task starts. A task does not
  start on a Proposed ADR.
- The `value_time_splits` table holds 0 rows on the primary. The backup of
  2026-10-19 gave 0.
- The crawler depends on the parser through the path `../stophammer-parser`.
  A parser change and its crawler change build together.

## Affected Modules

| Repository | Module | Tasks |
|---|---|---|
| stophammer-crawler | `Cargo.toml`, `src/crawl.rs`, `src/modes/batch.rs`, `src/modes/import.rs` | 0072-001, 0072-002, 0072-003 |
| stophammer-crawler | `src/crawl.rs`, `src/feed_skip.rs`, `src/modes/import.rs`, `src/modes/ndjson.rs` | 0074-002 |
| stophammer-parser | `src/error.rs`, `src/engine.rs`, `src/profile.rs`, `src/bin/stophammer-parse.rs` | 0074-001, 0074-003 |
| stophammer-parser | `src/types.rs`, `src/engine.rs` | 0073-003 |
| stophammer | `migrations/`, `src/schema.sql`, `src/db.rs`, `src/model.rs`, `src/ingest.rs`, `src/event.rs`, `src/apply.rs`, `src/api.rs` | 0073-001 |
| stophammer | `src/query.rs`, `src/openapi.rs`, `docs/API.md` | 0073-002 |

## Sequence

Three tracks can run at the same time. In a track, each task waits for
the task before it.

1. **Crawler fetch track.** ADR 0072 tasks 001, 002 and 003.
2. **GUID track.** ADR 0074 task 001 (parser), task 002 (crawler), task 003
   (parser).
3. **Split track.** ADR 0073 task 001 (node), task 002 (node), task 003
   (parser).

Two tasks change `src/crawl.rs`: ADR 0072 task 001 and ADR 0074 task 002. Run
them one after the other, in either order. ADR 0074 task 003 and ADR 0073
task 003 both change the parser. Run ADR 0073 task 003 after ADR 0074 task
003.

## Schema And API Implications

- Migration 0053 rebuilds `value_time_splits` and adds
  `value_time_split_recipients` (ADR 0073 §2). Check the next free number when
  the task starts.
- `TrackUpsertedPayload` changes its split shape. This is a change to the event
  protocol (ADR 0073 §3).
- The ingest DTO accepts the old split shape and the new one.
- `GET /v1/feeds/{guid}/tracks/{track_guid}` and `GET /v1/tracks/{guid}` with `include=value_time_splits` add fields. The new
  include `local_value_time_splits` is added. No `v1` field changes (ADR 0044
  §4).
- The parser public API loses the fallback GUID (ADR 0074 §2). The parser CLI
  loses its fallback argument.

## Release

One MINOR release (ADR 0066) holds the three tracks. The release notes say that
community nodes upgrade first, because of ADR 0073 §3.

1. Community nodes take the release.
2. The primary takes the release.
3. The gossip crawler and each other crawler take the release, after the
   primary.
4. The operator crawls the Satellite Skirmish feed with the `feed` mode, and
   reads its tracks with `include=value_time_splits`. Three remote-form rows
   are the expected result.
5. After the first `refresh` pass, the operator reads the report line of ADR
   0072 §6. The two Apache hosts of ADR 0072 give `304`, not wasted `200`
   answers.

## Risk Areas

- **The event protocol.** An old community node cannot apply a local-form split.
  The release notes must say this.
- **The skip records.** A wrong skip rule hides a feed with no time limit.
  ADR 0074 task 002 tests each row of §3b in both stores.
- **The `304` answers.** A wrong `If-None-Match` list makes each fetch a `200`.
  The counter of ADR 0072 task 003 shows it after the first pass.
- **The parser API.** ADR 0074 task 003 removes public functions. The parser
  CLI and the crawler must not use them after task 002.

## Test Strategy

- Each task keeps the pre-commit gate green in each crate that it changes.
- No test sends a request to an external host. The crawler tests use the local
  HTTP stub of `src/crawl.rs`.
- The six parser tests and the two crawler tests of the research request are
  acceptance tests. Their files are in
  `~/build/musicindex-research/sources/captures/2026-10-20-stophammer-claim/tests/`.

## Rollback

Each task is one commit in one repository, and reverts alone.

- A revert of ADR 0072 task 001 removes compression. The cache rows stay
  correct, because each row holds a decoded body.
- A revert of ADR 0074 task 002 restores the fallback. The skip rows that the
  task wrote stay correct.
- Migration 0053 is not reverted. A revert of ADR 0073 task 001 needs a new
  migration that restores the old table. The table holds no row today, so the
  cost is low.
