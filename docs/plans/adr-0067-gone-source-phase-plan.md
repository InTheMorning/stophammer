# ADR 0067 Phase Plan: A Gone Source Retires Its Feed

Date: 2026-09-27. This plan states no rule.
[ADR 0067](../adr/0067-a-gone-source-retires-its-feed.md) owns each rule.

## Goal

The crawler reports a gone answer from a source URL. The node counts the
answers and retires the record on the second one, 24 hours or more after the
first. The operator sets `SOURCE_GONE_HOSTS=wavlake.com`.

## Non-Goals

The non-goals of ADR 0067. Also: no change to the ingest request type, no new
event type, and no change in `stophammer-parser`.

## Facts

From the code on 2026-09-27:

- `IngestFeedRequest.feed_data` is already `Option<IngestFeedData>`
  (`src/ingest.rs`).
- `feeds.feed_url` is `UNIQUE`. `feed_indexed_by_stored_url` in `src/db.rs`
  finds the record of a URL.
- The ADR 0057 retirement in `src/api.rs` calls `db::delete_feed_with_event`
  with a signed `FeedRetired` payload and no block. That is the transaction of
  ADR 0053 task 003.
- In `stophammer-crawler/src/crawl.rs`, the branch `status != 200` returns a
  `FetchError` report with the status, the final URL and the redirects. It
  sends nothing to the node.
- `FeedSkipDb::record_outcome` keeps only `200` answers, and the gossip feed
  memory skips only by medium. A `404` thus never makes a skip, and the next
  pass fetches the URL again.
- `CrawlConfig` is built in `modes/batch.rs` (`feed`, `refresh`),
  `modes/gossip.rs`, `modes/import.rs` and `modes/ndjson.rs`.

## Plan Decisions

1. **The node table.** `source_gone_answers (feed_guid TEXT PRIMARY KEY
   REFERENCES feeds(feed_guid) ON DELETE CASCADE, source_url TEXT NOT NULL,
   first_gone_at INTEGER NOT NULL, last_gone_at INTEGER NOT NULL,
   last_status INTEGER NOT NULL) STRICT`. The cascade deletes the row with its
   record, so the feed delete trigger does not change.
2. **Migration 0045**, at array position 39. ADR 0056 task 002 becomes
   migration 0046, and ADR 0034 task 003 becomes migration 0047.
3. **The node branch** runs after the crawl token check and before the
   verifier chain, when `feed_data` is `None` and `http_status` is `404` or
   `410`. A request with no `feed_data` and another status keeps its present
   behavior.
4. **The crawler flag** is `CrawlConfig.report_gone`, default false, with a
   builder `with_report_gone`. `batch.rs` and `gossip.rs` set it. `import.rs`
   and `ndjson.rs` do not.
5. **The crawler outcome does not change.** A gone answer stays a
   `FetchError` in the report and in the batch counts. The crawler logs the
   reason that the node gives.

## Affected Modules

| Task | Repository | Module | Change |
|---|---|---|---|
| 001 | `stophammer` | `api`, `db`, `main`, `migrations`, `schema.sql`, `openapi` | The table, the setting, the branch, the reasons |
| 002 | `stophammer-crawler` | `crawl`, `modes/batch`, `modes/gossip` | The report |
| 003 | None | Deploy | The setting, and the checks |

## Sequence

| Task | Repository | Goal | Needs |
|---|---|---|---|
| [001](../tasks/adr-0067-task-001-node-counts-gone-answers.md) | `stophammer` | The node counts gone answers and retires | None |
| [002](../tasks/adr-0067-task-002-crawler-reports-gone-answers.md) | `stophammer-crawler` | The crawler reports a gone answer | None |
| [003](../tasks/adr-0067-task-003-deploy.md) | All | Release 0.3.0 and the checks | 001, 002 |

Task 001 and task 002 change different repositories and can run at the same
time. A crawler with task 002 and a node without task 001 is safe: the node
rejects the report, and nothing changes.

## Schema And API

- Migration 0045 adds `source_gone_answers`. `src/schema.sql` gets the same
  table.
- The ingest response gets three reasons: `source_gone_observed`,
  `source_gone` and `source_gone_ignored`. `FeedRetired` gets the reason
  `source_gone`. `docs/API.md` and the OpenAPI document give them, and the
  guards of ADR 0044 check the document.
- `SOURCE_GONE_HOSTS` goes in `docs/operations.md` and in
  `packaging/env/primary.compose.env.example`.

## Risk Areas

- **A wrong retirement.** A host fault that answers `404` for two days
  retires each feed that a crawl reaches in that time. The feeds come back on
  their next crawl with a body. Task 003 checks the count of retirements
  after the first pass.
- **The verifier chain.** A request with no `feed_data` must not reach a
  verifier that expects a body. The branch returns before the chain.
- **A community node.** It gets only the signed `FeedRetired`, which it
  applies today. It keeps no `source_gone_answers` row.

## Test Strategy

- Task 001: integration tests for each guard of ADR 0067, with the clock set
  by the test, and the migration test.
- Task 002: a test against a local HTTP stub on `127.0.0.1` that answers
  `404`, and checks the report that the crawler posts to a stub node. A test
  that an `import` config sends no report. No test sends a request to an
  external host.

## Rollback

- The node: go back to the previous image. The table stays, and the old code
  ignores it. A retired feed comes back on its next crawl with a body.
- The crawler: go back to the previous image. The node then gets no report.
