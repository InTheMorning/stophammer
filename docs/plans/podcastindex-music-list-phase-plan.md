# Phase Plan: A Daily Pass Reads The PodcastIndex Music List

This plan is advisory. It gives the sequence of the tasks of
[ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md),
which owns each rule.

## Goal

- A crawler mode `podcastindex` reads the PodcastIndex music list in one API
  request. It compares the list with the feeds that the node holds, and
  crawls each feed that the node does not hold.
- The mode keeps the result of each candidate, and tries a failed candidate
  again after 7 days.
- Each pass measures the feeds that `import` accepted and that the music list
  does not hold (§10).
- The operator can run the pass each day with Compose or systemd.

## Non-Goals

- Dead feeds. ADR 0067 stays the only path that retires a gone source.
- Publisher and `musicL` feeds. The follow waves find them.
- Changes to held feeds. `refresh` (ADR 0047) gets them.
- A change to the node, the parser, or the HTTP contract.
- A stop of the `import` schedule. §10 measures first, and a later ADR
  decides.

## Assumptions

- ADR 0081 is Accepted before a task starts. A task does not start on a
  Proposed ADR.
- The open questions take the proposed values:
  - the mode is `podcastindex`,
  - a schedule of the operator starts each pass,
  - a failed candidate waits 7 days,
  - the limits are 1,000 rows, 20,000 rows and 2,000 candidates,
  - the measurement runs 28 days.

  A different answer changes the named constant in the task that holds it.
- The API response has the shape that the probe runs of 2026-10-31 saw: a
  JSON object with `status`, `feeds` and `count`, and each row with `id`,
  `url` and `podcastGuid`.
- The `sha1` crate of RustCrypto builds with the `digest` version that `sha2`
  already uses, so the lock file gets one new crate.
- `GET /v1/feeds/recent` gives `feed_guid` and `feed_url` on each row. The
  response of 2026-10-31 confirms this.

## Affected Modules

All tasks are in the `stophammer-crawler` repository, except task 006, which
also changes files of `stophammer`.

| Module | Tasks |
|---|---|
| `src/podcastindex_api.rs`, new | 001 |
| `Cargo.toml`, `Cargo.lock` (the `sha1` crate) | 001 |
| `src/modes/refresh.rs` (the node list reader becomes `pub(crate)`) | 002 |
| `src/modes/podcastindex.rs`, new | 002, 003, 004, 005 |
| `src/modes/import.rs` (`open_state_connection` becomes `pub(crate)`) | 003 |
| `src/modes/batch.rs` (an outcome sink on the pipeline) | 004 |
| `src/main.rs`, `src/modes/mod.rs` | 004 |
| `README.md`, `AGENTS.md` of the crawler | 006 |
| `stophammer`: `docker-compose.yml`, `packaging/`, `docs/operations.md`, `AGENTS.md` | 006 |

## Sequence

1. **Task 001, the API client.** The credentials, the `Authorization` header,
   the request and the parse of the list. No other module depends on it
   before task 004.
2. **Task 002, the comparison.** The node list with GUIDs, the URL
   normalization and the classification of each row. It changes
   `src/modes/refresh.rs` and starts `src/modes/podcastindex.rs`.
3. **Task 003, the attempts and the wait.** The table `music_list_attempts`
   and the 7-day rule. It can run at the same time as task 002 only when the
   two agents do not edit `src/modes/podcastindex.rs` at the same time. Run
   them in sequence when one agent does both.
4. **Task 004, the pass and the CLI.** The outcome sink in `batch`, the limits
   of §6, the report of §8, `--dry-run` and the `podcastindex` subcommand. It
   starts after tasks 001, 002 and 003.
5. **Task 005, the measurement of §10.** It starts after task 004, because it
   reads the pass start time that task 004 stores.
6. **Task 006, the packaging and the documents.** It starts after task 004.
   It can run at the same time as task 005.

## Interim Behavior

- After tasks 001 to 003, the binary has no new subcommand. The new code is
  reached only by tests.
- After task 004, the mode runs and records attempts, and logs no §10 count.
- After task 005, the first pass logs "no previous pass" and counts nothing.
  The second pass gives the first count.

## Schema And API Implications

- `import_state.db` gets three tables: `music_list_attempts`,
  `import_gap_passes` and `import_gap_rows`, and one `import_progress` key,
  `podcastindex_pass_started_at`. `import` reads none of them. A downgrade of
  the crawler leaves the tables in place, and `import` does not fail on them.
- The node, its schema and `GET /v1/feeds/recent` do not change.
- `POST /ingest/feed` gets the same request body as from the `feed` mode.
- The crawler release is MINOR (ADR 0066). The node has no release.

## Risk Areas

- **The credentials in a log.** `reqwest::Error` prints the URL and not the
  headers. The mode never formats the header map, the credentials struct
  redacts its `Debug`, and no `eprintln!` takes a credential value.
- **The API shape.** A change of the field names gives a parse error, and §6
  stops the pass. The log names the error.
- **The body size.** The list was 9.8 MB on 2026-10-31. The client reads at
  most 64 MiB and gives an error above that.
- **`too_many_arguments`.** `batch::run_urls` has 7 parameters. Task 004 adds
  the sink through a new function with an options struct, and keeps
  `run_urls` as a wrapper, so no caller changes.
- **Two writers of `import_state.db`.** The file uses the `MEMORY` journal
  mode and a 5-second busy timeout. A pass that runs while `import` runs can
  get `SQLITE_BUSY` after 5 seconds. The pass stops with the error, and the
  schedule gives a different hour to the two jobs.
- **The URL normalization.** A match by URL after the removal of `www.` can
  join two different feeds on hosts where `www.` is a different site. The GUID
  match runs first, and the probe found no such case on 2026-10-31.
- **A redirect.** The pipeline follows redirects, and `CrawlReport.final_url`
  can differ from the candidate URL. The attempt row keys on the candidate
  URL that the mode sent, never on `final_url`.

## Test Strategy

- Each test of the "Verification" section of ADR 0081 is an inline
  `#[cfg(test)]` test in the crawler. Each task names its tests.
- HTTP tests use a `tokio::net::TcpListener` stub on `127.0.0.1`, as the
  tests of `src/crawl.rs` and `src/modes/batch.rs` do. No test reaches the
  real API or the real node.
- The gate of `stophammer-crawler/AGENTS.md` runs after each task:
  `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt -- --check`.
- The manual check of ADR 0081 runs after task 006: a `--dry-run` pass on the
  VPS gives the numbers of §8, and the operator compares them with the
  Context of the ADR.

## Rollback Strategy

- Each task is one commit, and a revert of that commit undoes it.
- The three tables stay in `import_state.db` after a revert. No other mode
  reads them.
- A pass that sent URLs to the pipeline is not undone. The node holds the
  accepted feeds, as after a `feed` run.

## After The Tasks

The orchestrator reviews each diff against
[the review checklist](../reviews/podcastindex-music-list-review-checklist.md).
A release record gives the crawler release. After the deploy, the
orchestrator deletes this plan and the task packets, and `AGENTS.md` keeps the
§10 measurement in "Open Items" until the operator decides.
