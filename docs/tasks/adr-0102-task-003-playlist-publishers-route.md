# ADR 0102 Task 003: The Route Of The Publisher Feeds With Playlists

Owner: [ADR 0102](../adr/0102-a-publisher-view-shows-its-playlists.md) §6 and
§7.

Repository: `stophammer`. Start this task only after task 002 is committed,
because the three tasks change `src/query.rs`.

## Goal

`GET /v1/feeds/playlist-publishers` gives each publisher feed with one or more
confirmed playlists, newest playlist first. `GET /v1/node/capabilities` names
`playlist_links`.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Route table | extend | `src/query.rs` | `query_routes` gets the new route |
| Recent list handler | reuse | `src/query.rs` | `handle_get_recent_feeds` gives the page form, the `limit + 1` read and the `publisher` row with `include=link_facts` |
| Cursor helpers | reuse | `src/query.rs` | `encode_cursor_for` and `decode_cursor_for` |
| List limit | reuse | `src/query.rs` | `LIST_LIMIT_MAX`, `ListQuery` and `ListQuery::capped_limit` |
| Page envelope | reuse | `src/query.rs` | `Pagination` and `QueryResponse` |
| Publisher rows and counts | reuse | `src/query.rs` | `load_publisher`, `is_confirmed_link` and the count of task 002 |
| Capabilities | extend | `src/query.rs` | `handle_capabilities` gives the list of names |
| OpenAPI document | extend | `src/openapi.rs` | Each router route must be in the document, and `limit_query_param` gives the limit |
| Route guards | reuse | `tests/adr0044_contract_guard_tests.rs` | They fail when a router route is not in the document |
| Capability tests | extend | `tests/client_requests_capabilities_tests.rs` | They read `GET /v1/node/capabilities` |

## Files To Inspect

- `docs/adr/0102-a-publisher-view-shows-its-playlists.md`: §6 and §7
- `src/query.rs`:
  - `query_routes` (near line 7660)
  - `Pagination` (near line 39), the cursor helpers (near line 44 to 90) and
    `QueryResponse` (near line 133)
  - `LIST_LIMIT_MAX`, `ListQuery` and `ListQuery::capped_limit` (near line 158)
  - `parse_recent_cursor` (near line 4842), `recent_feed_rows` (near line 4874)
    and `handle_get_recent_feeds` (near line 5161)
  - `CapabilitiesResponse` and `handle_capabilities` (near line 7175)
- `src/schema.sql`: `feed_remote_items_raw` and `feeds`
- `src/model.rs`: `Feed::release_date` and `Feed::last_build_date`
- `src/openapi.rs`: the route of `GET /v1/feeds/recent` (near line 155) and
  `limit_query_param` (near line 1519)
- `tests/adr0044_contract_guard_tests.rs`
- `tests/client_requests_capabilities_tests.rs`
- `tests/adr0092_release_track_counts_tests.rs`:
  `capabilities_list_release_track_counts`

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`
- `tests/fixtures/v1-schema-fields.json`, only for a new row type
- `tests/client_requests_capabilities_tests.rs`
- New: `tests/adr0102_playlist_publishers_route_tests.rs`

## Do Not Touch

- The rows of task 001 and the counts of task 002. Only read them.
- `src/db.rs`, the migrations and `src/schema.sql`. The route stores no result.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the ADR index and `AGENTS.md`.

## Constraints

- One SQL query selects the candidates: each publisher feed with a channel
  item with the medium `musicL`. Compare the medium with the ASCII case
  ignored. Write the query in `src/query.rs` with the other list queries
  (§6).
- For each candidate, compute the confirmed playlist links at read time with
  `load_publisher` and `is_confirmed_link`. Keep only the candidates with a
  count of 1 or more.
- Each row is the `publisher` row of `GET /v1/feeds/recent` with
  `include=link_facts`, with `two_way_playlist_link_count` and
  `newest_playlist_at` (§6).
- `newest_playlist_at` is the newest date of the confirmed playlists, in Unix
  seconds. The date of one playlist is its `release_date`, else its
  `last_build_date`. The value is null when no confirmed playlist has a date.
- Sort by `newest_playlist_at`, newest first. A null value comes last.
  `feed_guid` sorts rows with the same value. The cursor holds the two sort
  values, in the form of `encode_cursor_for`.
- The route takes `limit` and `cursor`. The maximum of `limit` is
  `LIST_LIMIT_MAX`. Use the error of `GET /v1/feeds/recent` for a bad cursor.
- `GET /v1/node/capabilities` names `playlist_links` in `capabilities` (§7).
- Each new comment names ADR 0102 and its section.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Write each comment and each document line in Simplified Technical English.
  Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

1. A router test: two publisher feeds with confirmed playlists of different
   dates and one publisher feed with only one-way playlist links.
   `GET /v1/feeds/playlist-publishers` gives the two feeds with confirmed
   playlists, newest `newest_playlist_at` first. The third feed is not in the
   list (ADR 0102 acceptance 6).
2. A router test: a playlist with no `release_date` uses its
   `last_build_date`. A feed whose confirmed playlists have no date comes
   last, with `newest_playlist_at` null.
3. A router test: `limit=1` gives one row and a cursor. The next page with
   that cursor gives the second row.
4. A test: `GET /v1/node/capabilities` names `playlist_links` (ADR 0102
   acceptance 7).
5. `tests/adr0044_contract_guard_tests.rs` passes, so the OpenAPI document
   names the new route. The document gives the row schema with
   `two_way_playlist_link_count` and `newest_playlist_at` (ADR 0102
   acceptance 8).
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
