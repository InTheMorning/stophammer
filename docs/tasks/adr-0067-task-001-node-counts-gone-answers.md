# ADR 0067 Task 001: The Node Counts Gone Answers

Owner: [ADR 0067](../adr/0067-a-gone-source-retires-its-feed.md). Plan:
[the phase plan](../plans/adr-0067-gone-source-phase-plan.md).

Repository: `stophammer`. The operator commits.

## Goal

The node accepts an ingest request with no `feed_data` and the status `404` or
`410`. It keeps the first gone answer of the record. A gone answer 24 hours or
more after the first retires the record, with the reason `source_gone`.

## Files To Inspect

- `src/ingest.rs`: `IngestFeedRequest`, `IngestResponse`
- `src/api.rs`: the start of the ingest handler (the crawl token check and
  the verifier chain), and the ADR 0057 retirement that calls
  `db::delete_feed_with_event` with the reason `podcast_block` (search for
  `podcast_block`)
- `src/db.rs`: `feed_indexed_by_stored_url`, `delete_feed_with_event`,
  `MIGRATIONS`, and the write of a feed at ingest
- `src/main.rs`: where the node reads its env settings into `AppState`
- `src/openapi.rs`: the description of the ingest response reasons
- `docs/API.md`: the ADR 0057 text near `podcast_block`
- `docs/operations.md`, `packaging/env/primary.compose.env.example`,
  `packaging/env/primary.env.example`
- `tests/`: the ADR 0057 tests, for the helpers that post an ingest request
  (search for `source_blocked`)

## Files Likely To Change

- `migrations/0045_source_gone_answers.sql`, new
- `src/schema.sql`, `src/db.rs`, `src/api.rs`, `src/main.rs`, `src/openapi.rs`
- `docs/API.md`, `docs/operations.md`, `docs/schema-reference.md`, the two env
  example files
- `tests/adr0067_source_gone_tests.rs`, new
- `tests/migration_tests.rs`

## Do Not Touch

- The ingest request type and the event types.
- The ADR 0057 rule, the ADR 0053 blocks, and the feed delete trigger.
- `stophammer-crawler/`, `stophammer-parser/`.

## Constraints

### Storage

- The table of plan decision 1, in migration 0045 at array position 39, and
  in `src/schema.sql`. The row is local to the primary and makes no event.
- An ingest that writes a body for a record deletes its row. Do this at the
  place where the ingest writes the feed, in the same transaction.

### The setting

- `SOURCE_GONE_HOSTS`: a comma-separated list of host names. Trim each name,
  lowercase it, and drop an empty name. Unset or empty means no host.
- Read it once at start into `AppState`, as the node reads its other
  settings. Give tests a way to set it without the process environment.
- A host matches when the host of the URL is equal to a listed name. A
  subdomain does not match.

### The branch

Put it after the crawl token check and before the verifier chain. It runs only
when `feed_data` is `None` and `http_status` is `404` or `410`. Each other
request keeps its present behavior.

1. The report counts only when `source_url` and `canonical_url` are equal,
   `redirects` is empty, and `feed_indexed_by_stored_url` finds a record for
   the URL. A `404` counts only when the host is listed. A `410` counts from
   any host.
2. A report that does not count gets `accepted: false`, the reason
   `source_gone_ignored`, and changes nothing.
3. A counted report with no row writes the row, with `first_gone_at` and
   `last_gone_at` set to now. The response is `accepted: false` with the
   reason `source_gone_observed`.
4. A counted report with a row, less than 24 hours after `first_gone_at`,
   updates `last_gone_at` and `last_status`. The response is the same as in
   rule 3.
5. A counted report with a row, 24 hours or more after `first_gone_at`,
   retires the record. Use `db::delete_feed_with_event` with a signed
   `FeedRetired` payload, the reason `source_gone`, and no block, as the ADR
   0057 retirement does. The cascade deletes the row. The response is
   `accepted: false` with the reason `source_gone` and the event ID in
   `events_emitted`. Fan out the event as the ADR 0057 path does.
6. Log each result with `tracing::info!`, with `feed_guid`, the URL, the
   status and the reason.
7. Name the 24 hours as one constant with a doc comment that names ADR 0067
   §3. Take the time from the same clock that the ingest uses, so a test can
   set it.

### The contract

- `docs/API.md` and the ingest response description in `src/openapi.rs` give
  the three reasons, and the `FeedRetired` reason `source_gone`.
- `docs/operations.md` and the two env example files give
  `SOURCE_GONE_HOSTS`, empty in the examples, with a comment that the
  operator of `api.musicindex.org` sets `wavlake.com`.

## Acceptance

Mechanical, each an integration test in `tests/adr0067_source_gone_tests.rs`:

- A `404` for a stored source URL at a listed host, then a second one 24
  hours later, retires the record. The second response has the reason
  `source_gone` and one event. The log has a `FeedRetired` event with
  `source_gone`.
- A second `404` 23 hours after the first does not retire the record.
- A `404`, then an ingest of a body for the record, then a `404` 24 hours
  after the first: no retirement. The body deleted the row.
- A `404` from a host that is not listed gets `source_gone_ignored`, and no
  row.
- A `410` from a host that is not listed counts, and the second one retires.
- A report for a URL that is no stored source URL gets
  `source_gone_ignored`.
- A report with a redirect gets `source_gone_ignored`.
- A request with no `feed_data` and the status `500` keeps its present
  response.
- `api.musicindex.org` is not named in a test.
- The migration test: a database at position 38 migrates to 39 and has the
  table.
- The guards of ADR 0044 pass. The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0067_source_gone_tests
cargo test --test migration_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- The ingest handler checks the crawl token after the verifier chain.
- A request with no `feed_data` and the status `404` already has a meaning
  in the code.
- `delete_feed_with_event` cannot run without a feed body.
- A migration other than 0044 is the last file when you start.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0067-task-001-node-counts-gone-answers.md
- /home/citizen/build/stophammer/docs/adr/0067-a-gone-source-retires-its-feed.md
- /home/citizen/build/stophammer/docs/plans/adr-0067-gone-source-phase-plan.md (Facts, Plan Decisions)
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests, New Database Migration)
- Only the parts of src/api.rs and src/db.rs named in the task file. Use grep. The files are long.

Goal:
- The node counts gone answers from a stored source URL, and retires the record with the reason source_gone on the second answer 24 hours or more after the first.

Constraints:
- The rules under "Constraints" in the task file.
- Write any scratch file under target/, not /tmp.

Do not touch:
- The ingest request type, the event types, the ADR 0057 rule, the ADR 0053 blocks, the feed delete trigger, stophammer-crawler/, stophammer-parser/.
- Git: run no git command that writes (no add, commit, stash, checkout, reset, push).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
