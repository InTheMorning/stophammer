# ADR 0058 Task 006: A Copy Row Gives Its Item Titles And Image

Owner: [ADR 0058](../adr/0058-a-copy-of-a-feed-is-public.md) section 1c and
section 3. musicindex.org request 9. Release 0.5.0.

Repository: `stophammer`. The operator commits.

## Goal

Each row of `GET /v1/feeds/{guid}/copies` gives `item_guids`, `item_titles` in
the same sequence, and `image_url`. A change of the channel title, an item
title or the image signs one `FeedCopyObserved` event. It does not change the
summary digest, so a resolution holds.

## Files To Inspect

- `src/model.rs`: `CopySummary`, `copy_summary`, `summary_digest` and
  `CopySummaryDigestFields`
- `src/api.rs`: `record_feed_copy_for_ingest`, `feed_copy_observed_event_row`
- `src/event.rs`: `FeedCopyObservedPayload`
- `src/apply.rs`: the `FeedCopyObserved` arm
- `src/db.rs`: `FeedCopyRow`, `get_feed_copy`, `list_feed_copies`,
  `upsert_feed_copy_summary`, the `MIGRATIONS` array
- `src/query.rs`: `FeedCopyResponse`, `handle_get_feed_copies`,
  `web_url_or_none`
- `src/openapi.rs`: the schema of the copy row
- `tests/adr0058_ingest_copy_tests.rs`, `tests/adr0058_copy_routes_tests.rs`,
  `tests/adr0058_resolve_tests.rs`: the helpers

## Files Likely To Change

- `migrations/0049_copy_titles_and_image.sql`, new, and `src/schema.sql`
- `src/model.rs`, `src/api.rs`, `src/event.rs`, `src/apply.rs`, `src/db.rs`,
  `src/query.rs`, `src/openapi.rs`
- `tests/migration_tests.rs`: the recorded version becomes 43
- `tests/adr0058_copy_titles_tests.rs`, new
- `docs/API.md`: the copies route

## Do Not Touch

- `summary_digest` and `CopySummaryDigestFields`. The digest keeps its fields.
- The resolution, the relocation and the limit of 20 rows.
- `stophammer-crawler`, `stophammer-parser`. The ingest data already holds the
  item titles and the channel image.

## Constraints

- **Storage.** Migration 0049 adds two nullable columns to `feed_copies`:
  `item_titles TEXT` (a JSON array, in the sequence of `item_guids`, each
  entry a string or null) and `image_url TEXT`. Null in `item_titles` means
  that the row has no titles yet. A row of a copy with no image keeps
  `image_url` null.
- **Summary.** `CopySummary` gets `item_titles: Vec<Option<String>>` and
  `image_url: Option<String>`, filled by `copy_summary` from the ingest data.
  An empty title gives null.
- **Event.** `FeedCopyObservedPayload` gets `item_titles:
  Option<Vec<Option<String>>>` and `image_url: Option<String>`, each with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`. A new event
  always sets `item_titles`. An old event has neither field, and applies as
  before with both columns null.
- **When to sign.** In `record_feed_copy_for_ingest`, a row that exists signs
  one event when the digest changed. It also signs one when the stored title,
  item titles or image differ from the new summary. Stored null item titles count as
  different, so an old row gets its values once. In each case the event
  carries the full summary, and the row stores it.
- **Response.** `FeedCopyResponse` gets `item_guids: Vec<String>`,
  `item_titles: Option<Vec<Option<String>>>` and `image_url: Option<String>`.
  `image_url` goes through `web_url_or_none`. Register each field in the
  OpenAPI schema of the row.
- **Docs.** `docs/API.md` names the three fields for the copies route.

## Deploy Step

After the deploy of the release, the operator fills the old rows (ADR 0058
§1c). On the VPS, in the compose directory:

```bash
docker compose run -d --name copy-backfill stophammer-crawler --no-revalidate refresh \
  --concurrency 3 --host-delay-ms 3000
docker logs copy-backfill 2>&1 | grep '^fetch:'
```

Then count the rows that still have no titles. A row whose URL no pass
reaches keeps null:

```bash
docker run --rm -v stophammer_primary-data:/node alpine:3.20 sh -c 'apk add -q sqlite && sqlite3 -readonly /node/stophammer.db "SELECT COUNT(*), SUM(item_titles IS NULL) FROM feed_copies;"'
```

## Acceptance Criteria

Mechanical, each an integration test in `tests/adr0058_copy_titles_tests.rs`:

- A mirror body with three items gives a row with the three item GUIDs and
  the channel image. The third item has no title, so `item_titles` gives null
  in the third place.
- A second body that changes only an item title signs one `FeedCopyObserved`
  event, and the row gives the new title. The row keeps its
  `summary_digest`.
- A copy with a `keep_source` resolution stays resolved after a body that
  changes only an item title or the image.
- A body with the same summary signs no event.
- A row written with null `item_titles` gets the titles at the next
  submission from its URL, with one event.
- A `javascript:` image gives `image_url` null in the response.
- An old `FeedCopyObserved` payload with neither new field applies, and the
  row gives `item_titles` null.
- A community node that applies the new event gives the same three fields.
- `tests/migration_tests.rs` records version 43. The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0058_copy_titles_tests
cargo test --test migration_tests
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

## Escalation Triggers

Stop and report when:

- An existing test counts the events of a mirror response and fails because
  of a new display event. List each one, and do not change it.
- The ingest data gives no channel image or no item title where this task
  expects one.
