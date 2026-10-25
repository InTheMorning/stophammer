# ADR 0080 Task 004: The Node Stores The Host Of Each Feed

Owner: [ADR 0080](../adr/0080-a-search-takes-phrases-and-field-filters.md)
§5, the `host` row of §2, `not_host` of §3, and §7. The phase plan is
[the search filters plan](../plans/search-filters-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
002. Run it after task 003 when one agent does both, because both tasks edit
the filter table.

## Goal

`feeds.feed_host` holds the host of `feed_url`. Each write of `feed_url` sets
it, and one repair fills the existing rows. `host` and `not_host` filter a
search by that host and its subdomains.

## Files To Inspect

- `src/db.rs`: `MIGRATIONS` (line 159), `try_open_db` and its repair calls
  (near line 797 to 860), `ensure_feed_title_sort_schema` (line 679) and
  `ensure_feed_title_sort_filled` (line 1019), `upsert_feed` (line 1342),
  `ingest_transaction` (line 6672)
- `src/api.rs`: `relocate_feed` (line 5294)
- `src/schema.sql`: `feeds`
- `src/search.rs` and `src/query.rs`: `SEARCH_FILTERS`, `SearchFilters`, the
  joins of task 003
- `docs/adr/0046-migration-versions-are-array-positions.md`
- `tests/migration_tests.rs`, `tests/fixtures/migration_sha256.txt`

## Files Likely To Change

- `migrations/0057_feed_host.sql`, new. Use the next free number if 0057 is
  taken.
- `tests/fixtures/migration_sha256.txt`: one new line
- `src/schema.sql`, `src/db.rs`, `src/api.rs`
- `src/search.rs`, `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/migration_tests.rs`: each recorded version moves by one
- `tests/adr0080_host_filter_tests.rs`, new

## Constraints

- **The host function.** Add `pub fn feed_host_of(feed_url: &str) -> String`
  in `src/db.rs`. It gives `url::Url::parse(feed_url)` and then `host_str()`,
  in lower case, with no trailing dot. A URL that does not parse, or that has
  no host, gives the empty text. The empty text means "no host", and the
  repair does not read the row again.
- **The migration.** `ALTER TABLE feeds ADD COLUMN feed_host TEXT;` and no
  index. ADR 0080 §5 gives the reason. The migration does not fill the column.
- **The schema.** `src/schema.sql` gets the column in `feeds`.
- **The repairs.** Follow the pattern of the `title_sort` repairs (ADR 0046):
  - `ensure_feed_host_schema` adds the column when `feeds` has no
    `feed_host`.
  - `ensure_feed_host_filled` sets `feed_host` from `feed_url` on each row
    where `feed_host` is NULL, in one transaction. It returns when no row is
    NULL. It logs one `tracing::info!` with the row count and the time.
  - Call the two repairs in `try_open_db`, adjacent to the `title_sort`
    repairs.
- **The writers.** `upsert_feed`, the feed insert of `ingest_transaction`, and
  `relocate_feed` each write `feed_host` from the new `feed_url`, in the same
  statement. No other statement writes `feed_url` now. Search again before you
  start, and add each new writer.
- **No model change.** `Feed` gets no field. No event payload changes. A
  community node calculates the host when it applies a feed.
- **The table.** Add `host` and `not_host` to `SEARCH_FILTERS`, for `feed`,
  `track` and `publisher`.
- **The value.** Reject a value that is empty, or that has `/`, `@`, or a `:`
  outside `[` and `]`, with `invalid_filter`. Then read the value with
  `url::Host::parse`. An error gives `invalid_filter`. Use the text of the
  parsed host, in lower case, with no trailing dot.
- **The match.** The condition reads `f.feed_host` of the join of task 003:
  `f.feed_host = ?n OR substr(f.feed_host, -length(?n) - 1) = '.' || ?n`.
  Thus `wavlake.com` matches `feeds.wavlake.com` and not `notwavlake.com`. The
  empty host does not match.
- **The exclusion.** `not_host` removes only a row whose host matches. A row
  with the empty host stays. `not_host` counts as an exclusion for the limit of
  10 (§3).
- **The contract.** Declare `host` and `not_host` on `/v1/search` in
  `src/openapi.rs`, and in `docs/API.md`.
- **ADR 0077.** The host does not come from a feed body, so no refill pass is
  necessary. Add no entry to `AGENTS.md`.
- **Comments.** Each new item names ADR 0080 §5.

## Do Not Touch

- The other filters and the error body, other than the table
- A search with no `q`. Task 005 adds it.
- The event payloads, `apply.rs` logic, and the `Feed` model
- Each existing migration file
- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the unit tests of `feed_host_of`.
2. Write the migration, its `MIGRATIONS` entry, its hash line and the change
   to `src/schema.sql`.
3. Write the two repairs and their calls.
4. Change the three writers.
5. Add the filter, its value parse and its condition.
6. Change the contract and the document.
7. Write the other tests.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0080_host_filter_tests.rs`, unless
the item names a different file.

- `feed_host_of` gives `feeds.wavlake.com` for
  `https://Feeds.Wavlake.com/feed/x`, `example.com` for
  `http://user@example.com:8080/a`, and the empty text for `not a url`.
- ADR 0080 check 11: an `upsert_feed`, an ingest of a new feed and a
  relocation each set `feed_host` from the new `feed_url`.
- Check 11: a database with a NULL `feed_host` on each row opens with a host
  on each row. A second open writes no row.
- Check 11: a database can record a migration version past 0057 and have no
  `feed_host` column. It opens with the column and a host on each row
  (ADR 0046).
- Check 12: `host=wavlake.com` matches a feed at `wavlake.com` and a feed at
  `feeds.wavlake.com`, and not a feed at `notwavlake.com`. A track matches by
  the host of its feed. A publisher matches by its own host.
- `not_host=wavlake.com` removes the feeds of `wavlake.com` and its
  subdomains, and keeps a feed with the empty host.
- Check 10, for this task: `host=`, `host=https://wavlake.com`,
  `host=wavlake.com/x`, `host=wavlake.com:443` and `host=a@b.com` each give
  `invalid_filter` with `filter` set to `host`.
- `host=Wavlake.COM.` matches the same feeds as `host=wavlake.com`.
- `tests/migration_tests.rs` passes with the new migration.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0080_host_filter_tests
cargo test --test migration_tests
cargo test --test adr0023_migration_hash_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A statement other than the three writers writes `feed_url`, and it is not
  simple to add `feed_host` to it.
- A community node writes a feed through a path that does not set
  `feed_host`.
- The repair takes more than 5 seconds on a copy of the backup of 2026-10-24.
- A step needs a change to an event payload, an ADR, or a file of "Do Not
  Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/AGENTS.md` (the code style, the migration steps and the pre-commit gate)
- `~/build/stophammer/docs/adr/0080-a-search-takes-phrases-and-field-filters.md`, §2, §3, §5 and §7
- `~/build/stophammer/docs/adr/0046-migration-versions-are-array-positions.md`
- `~/build/stophammer/docs/tasks/adr-0080-task-004-host-filter.md` (this task)
- `~/build/stophammer/docs/tasks/adr-0080-task-003-value-filters.md` (the joins)
- the files of "Files To Inspect" in this task

Goal:
- `feeds.feed_host` holds the host of `feed_url`. Each writer sets it, and one repair fills it. `host` and `not_host` filter a search by the host and its subdomains.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- The empty text means "no host". The column has no index.
- No model field and no event payload change.
- Stop at each escalation trigger of this task.

Do not touch:
- the other filters, the error body, a search with no `q`, the event payloads, the `Feed` model, each existing migration file
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `AGENTS.md`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --test adr0080_host_filter_tests`
- `cargo test --test migration_tests`
- `cargo test --test adr0023_migration_hash_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
