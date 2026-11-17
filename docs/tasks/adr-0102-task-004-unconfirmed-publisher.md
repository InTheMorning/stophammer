# ADR 0102 Task 004: A Playlist Row Names Its Unconfirmed Publisher

Owner: [ADR 0102](../adr/0102-a-publisher-view-shows-its-playlists.md) §5a.

Repository: `stophammer`. Tasks 001 to 003 are on `main`. This task changes
`src/query.rs` after them.

## Goal

Each row that gives `confirmed_publisher` also gives `unconfirmed_publisher`,
with no include. The value names the publisher reference of a playlist when
its `music_to_publisher` row is not a confirmed link.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Confirmed publisher read | extend | `src/query.rs` | `confirmed_publisher_for_feed` reads the same `music_to_publisher` row |
| Feed list rows | extend | `src/query.rs` | `feed_list_row_to_response` sets `confirmed_publisher` |
| Feed response | extend | `src/query.rs` | `FeedResponse` holds `confirmed_publisher` |
| Publisher row builder | reuse | `src/query.rs` | `load_publisher` gives the `music_to_publisher` row |
| Confirmed link test | reuse | `src/query.rs` | `is_confirmed_link` is the rule of ADR 0049 §6a |
| Schema field record | extend | `tests/fixtures/v1-schema-fields.json` | The record of each `v1` field (ADR 0044) |
| Playlist count tests | extend | `tests/adr0102_playlist_counts_tests.rs` | `setup` ingests a confirmed playlist and a playlist with no reference |

## Files To Inspect

- `docs/adr/0102-a-publisher-view-shows-its-playlists.md`: §5, §5a and
  acceptance 5a
- `src/query.rs`:
  - `FeedResponse` and its field `confirmed_publisher` (near line 752)
  - `ConfirmedPublisher` and `confirmed_publisher_for_feed` (near line 847)
  - `build_feed_response`, where `confirmed_publisher: None` is set (near
    line 2653)
  - `feed_list_row_to_response` (near line 5159)
  - `is_confirmed_link` and `PublisherRow`
- `src/openapi.rs`: `schemas_object`, if a new type needs a schema entry
- `tests/fixtures/v1-schema-fields.json`: `FeedResponse`
- `tests/adr0102_playlist_counts_tests.rs`
- `tests/adr0044_field_removal_tests.rs`

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`, only if a new type needs a schema entry
- `tests/fixtures/v1-schema-fields.json`
- `tests/adr0102_playlist_counts_tests.rs`

## Do Not Touch

- The value of `confirmed_publisher`. It must not change.
- `link_facts`, the counts, the route table and
  `GET /v1/node/capabilities`.
- `src/db.rs`, the migrations and `src/schema.sql`.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the ADR index and `AGENTS.md`.

## Constraints

- `unconfirmed_publisher` is an object with `feed_guid` and `title`. Make a
  new struct `UnconfirmedPublisher` with these two fields. `title` is
  nullable (§5a).
- Read the publisher rows one time for each feed. Extend
  `confirmed_publisher_for_feed`, or replace it with one function that gives
  the two values. Do not call `load_publisher` two times for one row.
- The `music_to_publisher` row is the row of the publisher reference. When
  that row is a confirmed link (`is_confirmed_link`), `confirmed_publisher`
  gets the current value and `unconfirmed_publisher` is null. When that row
  is not a confirmed link, `confirmed_publisher` is null and
  `unconfirmed_publisher` gives `publisher_feed_guid` and
  `remote_feed_title`. When the feed has no `music_to_publisher` row, the two
  fields are null.
- A row gives `unconfirmed_publisher` only when it gives
  `confirmed_publisher`: each `musicL` row of `GET /v1/feeds/recent` and
  `GET /v1/feeds/by-name`. Use the same omission rule (`Option<Option<..>>`
  with `skip_serializing_if`). `build_feed_response` sets the field to
  `None`, as it does for `confirmed_publisher`.
- Add `unconfirmed_publisher` to the `FeedResponse` entry of
  `tests/fixtures/v1-schema-fields.json`.
- Each changed comment names ADR 0102 §5a.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Write each comment and each document line in Simplified Technical English.
  Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

1. A router test on `GET /v1/feeds/recent?medium=musicL`: a playlist with a
   one-way link gives `unconfirmed_publisher` with the GUID and the title of
   the named publisher feed, and `confirmed_publisher` null (ADR 0102
   acceptance 5a). Add a playlist to `setup` that names the publisher feed,
   which the publisher feed does not list.
2. The same test: the confirmed playlist `playlist-one` gives
   `unconfirmed_publisher` null. Use `.get("unconfirmed_publisher") ==
   Some(&Value::Null)`, so that a missing field fails.
3. The same test: the playlist `playlist-one-way`, with no publisher
   reference, gives null in the two fields.
4. The same test: an album row does not give `unconfirmed_publisher`.
5. `tests/fixtures/v1-schema-fields.json` names `unconfirmed_publisher`, and
   `tests/adr0044_field_removal_tests.rs` passes (ADR 0102 acceptance 8, for
   this field).
6. Each existing test passes with no change of its assertions.

## Checks

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files and the result of each check. Name each point where the
code did not let you follow this packet.
