# ADR 0049 Task 004: URL Observations And A Stable `feed_url`

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §1.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decisions 1 to 4.
Needs: task 002 for the fixtures. Merge after task 003, because of the
migration number.

## Goal

1. The node records which URL gave which `podcast:guid`, and replicates each
   change through a signed `FeedUrlObserved` event.
2. A known GUID that arrives through a different URL keeps its stored
   `feed_url`.

## Files To Inspect

- `src/api.rs`: `handle_ingest_feed`. Read the reader phase, the
  `NO_CHANGE_SENTINEL` early return, the construction of `model::Feed`
  (`feed_url: req.canonical_url.clone()`), and how the write phase builds and
  signs the events
- `src/ingest.rs`: `IngestFeedRequest` (`canonical_url`, `source_url`)
- `src/event.rs`: `EventType`, `EventPayload`, `FeedRemoteItemsReplacedPayload`
  as a pattern
- `src/apply.rs`: the match on `EventPayload`
- `src/db.rs`: `MIGRATIONS`, `upsert_feed`, `get_feed`, `get_existing_feed`,
  `upsert_feed_crawl_cache`
- `src/verifiers/content_hash.rs`: the no-change check uses
  `feed_crawl_cache` by `canonical_url`
- `tests/event_tests.rs`: the list of event type tags
- `tests/apply_tests.rs`, `tests/migration_tests.rs`

## Files Likely To Change

- `migrations/0036_feed_url_observations.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/event.rs`, `src/apply.rs`, `src/api.rs`
- `tests/event_tests.rs`, `tests/migration_tests.rs`,
  `tests/adr0049_url_observation_tests.rs` (new)

## Do Not Touch

- `upsert_feed`. The stable URL comes from the `Feed` that ingest builds
- the verifier chain and `content_hash.rs`
- the publisher views in `src/query.rs`. Task 005 reads the observations
- `stophammer-crawler`, `stophammer-parser`

## Constraints

The table:

```sql
CREATE TABLE IF NOT EXISTS feed_url_observations (
    url         TEXT PRIMARY KEY,
    feed_guid   TEXT NOT NULL,
    observed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS idx_feed_url_observations_guid
    ON feed_url_observations(feed_guid);
```

- No foreign key to `feeds`. A retired feed keeps its observations. The
  resolver in task 005 checks that the GUID is indexed.
- The migration seeds the table:
  - `INSERT OR IGNORE INTO feed_url_observations (url, feed_guid, observed_at) SELECT feed_url, feed_guid, created_at FROM feeds;`.
- Add the same table to `src/schema.sql`. A fresh database has no feeds, so it
  needs no seed.

The record rule, as `db::record_feed_url_observation(conn, url, feed_guid, now)
-> Result<Option<FeedUrlObservation>, DbError>`:

- No row for `url`: insert it and return `Some`.
- A row with a different `feed_guid`: replace `feed_guid` and `observed_at`,
  and return `Some`.
- A row with the same `feed_guid`: write nothing and return `None`.

Ingest:

- On an accepted ingest that is not a no-change, record `canonical_url`. Also
  record `source_url` when it is different. Use the `feed_guid` of the feed
  data. Emit one `FeedUrlObserved` event for each `Some`, in the same
  transaction as the other events of that ingest.
- On the no-change path, also record the same URLs. The early return in the
  reader phase skips the writer, so the no-change path must take the writer
  for this step. Emit the events in the same way. Skip this step when the
  request has no feed data. The response stays `accepted: true, no_change:
  true`.
- On a rejected ingest, record nothing.
- When `db::get_feed(conn, feed_guid)` finds the GUID, the `Feed` that ingest
  builds uses the stored `feed_url`. It does not use `req.canonical_url`.

The event:

- `EventType::FeedUrlObserved`. The wire tag is `feed_url_observed`.
- `FeedUrlObservedPayload { url: String, feed_guid: String, observed_at: i64 }`.
- The subject GUID is `feed_guid`.
- Apply writes the row from the payload with an upsert on `url`. The primary
  node decides. Apply does not use the record rule.

## Implementation Steps

1. Add the migration, `MIGRATIONS` entry and `src/schema.sql` table.
2. Add `FeedUrlObservation`, `record_feed_url_observation` and
   `get_feed_url_observation` to `src/db.rs`.
3. Add the event type, payload and apply arm.
4. Record the observations in the write phase of ingest.
5. Record the observations on the no-change path.
6. Use the stored `feed_url` for a known GUID.
7. Add the tests below.
8. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A migration test proves that each existing feed has an observation of its
  `feed_url` with `observed_at == created_at`.
- A unit test proves each of the three branches of the record rule.
- Test, second URL: ingest `detox-album` with `canonical_url` A, then again with
  `canonical_url` B and a changed `content_hash`. The stored `feed_url` stays A.
  Both A and B have an observation of GUID
  `e5ac2d62-2ce7-517a-8bfd-24bff61b2aa9`.
- Test, redirect: ingest with `source_url` S different from `canonical_url` C.
  Both S and C have an observation.
- Test, no change: ingest a feed, then submit it again with the same
  `content_hash` and a new `source_url` S2. The response is `no_change: true`,
  and S2 has an observation.
- Test, no event on repeat: a second ingest from the same URL with the same GUID
  emits no `FeedUrlObserved` event.
- Test, reject: a rejected ingest records no observation.
- Test, replication: apply each `FeedUrlObserved` event from one database on a
  second database. The two `feed_url_observations` tables are equal.
- `tests/event_tests.rs` lists `feed_url_observed`.

## Test Commands

```bash
cargo build
cargo test --test migration_tests
cargo test --test adr0049_url_observation_tests
cargo test --test event_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
6. the count of events that one ingest emits before and after this task, for a
   feed ingest with no URL change

## Escalation Triggers

Stop and report when:

- the ingest code uses `existing` (the lookup by `canonical_url`) for more than
  the verifier context, and a second URL thus changes a stored value other than
  `feed_url`
- a GUID is indexed under a URL, and a different GUID arrives at that URL. The
  `UNIQUE` constraint on `feeds.feed_url` then decides the result. Report what
  the current code does. Do not change it
- the no-change path cannot take the writer without a change to the reader
  phase structure
- migration number 0036 is taken, or task 003 is not merged

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-004-url-observations.md`, the section
  "Constraints", in full
- `src/api.rs` `handle_ingest_feed`: the reader phase, the `NO_CHANGE_SENTINEL`
  return, the `model::Feed` construction, the event construction
- `src/event.rs`, `src/apply.rs`, and `FeedRemoteItemsReplacedPayload` as a
  pattern
- `src/db.rs`: `MIGRATIONS`, `get_feed`, `get_existing_feed`
- `src/verifiers/content_hash.rs`

Goal:
- Record which URL gave which `podcast:guid`, replicate each change with a
  signed `FeedUrlObserved` event, and keep the stored `feed_url` of a known
  GUID.

Constraints:
- Use the table, the seed, the record rule, the ingest rules and the event shape
  exactly as the section "Constraints" of the task file gives them.
- Record on accepted ingests and on the no-change path. Never on a rejected
  ingest.
- Record `canonical_url`, and `source_url` when it is different.
- A known GUID keeps its stored `feed_url` in the `Feed` that ingest signs. Do
  not change `upsert_feed`.
- Apply writes the observation from the payload with an upsert on `url`.

Do not touch:
- `upsert_feed`, the verifier chain, `content_hash.rs`
- the publisher views in `src/query.rs`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Tests prove:
  - The migration seeds each feed URL with `observed_at == created_at`.
  - The three record branches.
  - A second URL keeps `feed_url` A and gives observations for A and B.
  - A redirect gives observations for S and C.
  - A no-change submission with a new `source_url` records it.
  - A repeat from the same URL emits no `FeedUrlObserved` event.
  - A rejected ingest records nothing.
  - Replication gives equal tables.
  - `tests/event_tests.rs` lists `feed_url_observed`.

Test commands:
- `cargo build`
- `cargo test --test migration_tests`
- `cargo test --test adr0049_url_observation_tests`
- `cargo test --test event_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
