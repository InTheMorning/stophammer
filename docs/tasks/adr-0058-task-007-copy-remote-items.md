# ADR 0058 Task 007: A Copy Row Gives Its Feed List

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) section 1d,
section 2 and section 3. musicindex.org request 12. Release 0.8.0.

Repository: `stophammer`. The parser and the crawler do not change. The
operator commits.

## Goal

The summary of each copy keeps its channel-level `remoteItem` entries. Each
row of `GET /v1/feeds/{guid}/copies` gives them as `remote_items`, with the
resolved feed and its summary, and gives `differs_remote_items`. The node
compares the resolved feeds, not the raw `feedGuid` values.

## Files To Inspect

- `src/model.rs`: `CopySummary`, `copy_summary`, `summary_digest`,
  `CopySummaryDigestFields`
- `src/api.rs`: `record_feed_copy_for_ingest`, `feed_copy_observed_event_row`,
  the resolve handler that sets `resolved_digest`
- `src/event.rs`: `FeedCopyObservedPayload`, `FeedCopyResolvedPayload`
- `src/apply.rs`: the `FeedCopyObserved` and `FeedCopyResolved` arms
- `src/db.rs`: `FeedCopyRow`, `copy_differences`, `is_open_copy`,
  `open_copy_summary`, `upsert_feed_copy_summary`, `resolve_listed_feed`,
  `get_feed_remote_items_for_feed`, the `MIGRATIONS` array
- `src/ingest.rs`: `IngestRemoteFeedRef`
- `src/query.rs`: `FeedCopyResponse`, `handle_get_feed_copies`,
  `named_feed_summary`, `FeedRemoteItemResponse`, `web_url_or_none`
- `src/openapi.rs`: the schema of the copy row
- `tests/adr0058_copy_titles_tests.rs`: the pattern of task 006

## Files Likely To Change

- `migrations/0050_copy_remote_items.sql`, new, and `src/schema.sql`
- `src/model.rs`, `src/api.rs`, `src/event.rs`, `src/apply.rs`, `src/db.rs`,
  `src/query.rs`, `src/openapi.rs`
- `tests/migration_tests.rs`: the recorded version becomes 44
- `tests/adr0058_copy_remote_items_tests.rs`, new
- `docs/API.md`: the copies route

## Do Not Touch

- `summary_digest` and `CopySummaryDigestFields`. The summary digest keeps
  its fields.
- The limit of 20 rows, the relocation, and the URL block.
- `stophammer-crawler`, `stophammer-parser`. The ingest data already holds the
  channel-level remote items.

## Constraints

- **Storage.** Migration 0050 adds three nullable columns to `feed_copies`:
  `remote_items TEXT` (a JSON array in the sequence of the body),
  `remote_items_digest TEXT`, and `resolved_remote_items_digest TEXT`. Null
  `remote_items` means that the row has no list yet. A body with no
  channel-level item gives `[]`, not null.
- **Entry.** One stored entry holds `medium`, `feed_guid`, `feed_url` and
  `item_guid`, each the raw value from `IngestRemoteFeedRef` of
  `feed_data.remote_items`. `rel` is not a part of the entry.
- **List digest.** The SHA-256 hex of the JSON array of the different
  entries, sorted. The same entries in another sequence give the same
  digest.
- **Event.** `FeedCopyObservedPayload` gets `remote_items:
  Option<Vec<CopyRemoteItem>>`. `FeedCopyResolvedPayload` gets
  `resolved_remote_items_digest: Option<String>`. Each field has
  `#[serde(default, skip_serializing_if = "Option::is_none")]`.

  A new event always sets the field. An old event applies as before, with the column
  null. The apply step computes `remote_items_digest` from the list.
- **When to sign.** `record_feed_copy_for_ingest` signs one event when the
  summary digest changes, when a display value of section 1c changes, or
  when the list digest changes. A stored null list counts as a change, so an
  old row gets its list once.
- **Resolution.** A new resolution stores and signs the list digest of the
  row. `is_open_copy` takes the list digest into account: a resolution holds
  when `resolved_digest` equals `summary_digest`, and
  `resolved_remote_items_digest` is null or equals `remote_items_digest`.
- **Comparison.** `copy_differences` gives a third value,
  `differs_remote_items`. Resolve each entry of the row and each
  channel-level item of the record with `resolve_listed_feed`. The key of
  an entry is its resolved feed GUID and its `item_guid`. When an entry does
  not resolve, its key is its raw `feed_guid`, its `feed_url` and its
  `item_guid`.

  Compare the two sets of keys. A row with a null list gives
  false. `open_copy_summary` and `GET /v1/copies` use the third value.
- **Guard.** Never compare `remote_feed_guid` or `feed_guid` of an entry
  directly in `src/query.rs`. The ADR 0049 resolver guard fails.
- **Response.** `FeedCopyResponse` gets `differs_remote_items: bool` and
  `remote_items: Option<Vec<CopyRemoteItemResponse>>`. An entry gives
  `medium`, `feed_guid`, `feed_url` (through `web_url_or_none`),
  `item_guid`, `resolved_feed_guid`, and the summary of ADR 0059 from
  `named_feed_summary` for the resolved feed. Register each field in the
  OpenAPI schema of the row.
- **Docs.** `docs/API.md` names the new fields of the copies route, and the
  third difference.

## Deploy Step

Before the deploy, back up the database as for each release.

After the deploy, the operator sends the cached body of each copy URL again.
This needs no fetch, as for release 0.5.0. On the VPS, in the compose directory:

```bash
docker run --rm -v stophammer_crawler-data:/data -v stophammer_primary-data:/node -v "$PWD/stophammer-crawler/scripts:/scripts:ro" python:3.12-slim python /scripts/export-feed-cache-ndjson.py --cache /data/feed_cache.db --node-db /node/stophammer.db --output /data/copies.ndjson --copies
docker compose run --rm stophammer-crawler --force ndjson --input /data/copies.ndjson --state /data/copies_state.db --reset
```

Then count the rows that have no list, and the change of the open copies:

```bash
docker run --rm -v stophammer_primary-data:/node alpine:3.20 sh -c 'apk add -q sqlite && sqlite3 -readonly /node/stophammer.db "SELECT COUNT(*), SUM(remote_items IS NULL) FROM feed_copies;"'
curl -s "https://api.musicindex.org/v1/copies?limit=100" | python3 -c "import json,sys;print(len(json.load(sys.stdin)['data']))"
```

Before the deploy, 2 records had an open copy. Report the new count, and each
record that became open only by `differs_remote_items`.

## Acceptance Criteria

Mechanical, each an integration test in
`tests/adr0058_copy_remote_items_tests.rs`:

- The Longy case. The record is a publisher feed that lists album A by its
  `podcast:guid`. The copy lists album A by another `feedGuid` and the URL of
  A. The row gives `differs_remote_items` false, and the entry gives the
  resolved feed GUID and the title of A.
- A copy that lists album B, which the record does not list, gives
  `differs_remote_items` true, and `GET /v1/copies` lists the record.
- An album copy that names another publisher gives `differs_remote_items`
  true.
- Two `musicL` entries with the same `feedGuid` and different `itemGuid`
  values are two entries. A copy with one of them differs.
- A second body with the same entries in another sequence signs no event.
- A row written with a null list gets the list at the next submission from
  its URL, with one event. An old `keep_source` resolution still holds.
- A new `keep_source` resolution closes the copy. A later body that changes
  the list opens it again.
- An old `FeedCopyObserved` payload and an old `FeedCopyResolved` payload
  apply, and the row gives `remote_items` null.
- A community node that applies the new events gives the same
  `remote_items` and `differs_remote_items`.
- A `javascript:` `feedUrl` in an entry gives `feed_url` null.
- `tests/migration_tests.rs` records version 44. The ADR 0044 guards and the
  ADR 0049 resolver guard pass.

## Test Commands

```bash
cargo build
cargo test --test adr0058_copy_remote_items_tests
cargo test --test migration_tests
cargo test --no-fail-fast
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run -q --bin gen_openapi > /dev/null
```

## Expected Final Report

1. files changed
2. each acceptance criterion with the name of its test
3. tests run and their results
4. behavior changed
5. deviations from this task
6. unresolved concerns

## Escalation Triggers

Stop and report when:

- An existing test counts the events of a mirror response, or the open
  copies of a record, and fails because of the list. List each one, and do
  not change it.
- The ingest data of a `musicL` feed does not give the `itemGuid` of an
  entry.
