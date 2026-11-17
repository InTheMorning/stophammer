# ADR 0102 Task 002: The Playlist Count And The Curator Of A Playlist

Owner: [ADR 0102](../adr/0102-a-publisher-view-shows-its-playlists.md) §3 and
§5.

Repository: `stophammer`. Start this task only after task 001 is committed,
because the two tasks change `src/query.rs`. Task 003 starts after this task.

## Goal

Each row that gives `two_way_link_count` also gives
`two_way_playlist_link_count`. `GET /v1/feeds/{guid}` of a publisher feed gives
the count with no include. Each `musicL` row of `GET /v1/feeds/recent` and
`GET /v1/feeds/by-name` gives `confirmed_publisher`. No release count changes.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Link facts | extend | `src/query.rs` | `LinkFacts` and `link_facts` get the separate playlist count |
| Feed list rows | extend | `src/query.rs` | `feed_list_row_to_response` serves `GET /v1/feeds/recent` and `GET /v1/feeds/by-name` |
| Single feed read | extend | `src/query.rs` | `build_feed_response` reads the publisher rows of a publisher feed |
| Artist list and its cache | extend | `src/query.rs` | `ArtistNameRow`, `ArtistNameList` and `ArtistNameCacheEntry` hold `two_way_link_count` |
| Search rows | extend | `src/query.rs` | `SearchResponseItem` and `handle_search` give the link facts of a publisher row |
| Publisher row builder | reuse | `src/query.rs` | `load_publisher` gives the rows of task 001 |
| Confirmed link test | reuse | `src/query.rs` | `is_confirmed_link` is the rule of ADR 0049 §6a |
| OpenAPI document | extend | `src/openapi.rs` | The schemas come from the `ToSchema` structs |
| Schema field record | extend | `tests/fixtures/v1-schema-fields.json` | The record of each `v1` field (ADR 0044) |
| Link fact test helpers | reuse | `tests/adr0068_link_facts_tests.rs` | `stored_publisher` and `list_row` |
| Artist list test helpers | reuse | `tests/adr0071_artist_list_link_facts_tests.rs` | `artist_row` and `recent_row` compare the cached rows |
| Search test helpers | reuse | `tests/adr0076_search_link_facts_tests.rs` | `search_row` and `by_name_row` |

## Files To Inspect

- `docs/adr/0102-a-publisher-view-shows-its-playlists.md`: §3, §4 and §5
- `src/query.rs`:
  - `LinkFacts` and `link_facts` (near line 2306)
  - `confirmed_and_unconfirmed_release_artists` (near line 2267) and
    `co_credited_feeds` (near line 2364). These must not count a playlist.
  - `FeedResponse` (near line 660)
  - `build_feed_response` (near line 2539) and its publisher guard (near line
    2617)
  - `feed_list_row_to_response` (near line 5058), `handle_get_recent_feeds`
    (near line 5161) and `handle_get_feeds_by_name` (near line 5371)
  - `ArtistNameRow` (near line 5508), `with_link_facts`, `without_link_facts`,
    `ArtistNameList` (near line 5700), `build_artist_name_list` (near line
    5748), `ArtistNameCacheEntry` (near line 5958) and
    `ArtistNameCache::get_or_build` (near line 6009)
  - `SearchResponseItem` (near line 1169) and `handle_search` (near line 6559,
    the link facts near line 6879)
  - `handle_publisher_link_stats` (near line 7484). It must not change.
- `src/openapi.rs`: `schemas_object` and `response_schemas`
- `tests/fixtures/v1-schema-fields.json`: `ArtistNameRow`, `FeedResponse` and
  `SearchResponseItem`
- `tests/adr0044_field_removal_tests.rs`
- The three test files in the table

## Files Likely To Change

- `src/query.rs`
- `src/openapi.rs`, if a new nested type needs a schema entry
- `tests/fixtures/v1-schema-fields.json`
- New: `tests/adr0102_playlist_counts_tests.rs`

## Do Not Touch

- The rows and the facts of task 001. Only read them.
- The route table and `GET /v1/node/capabilities`. Task 003 changes them.
- `src/db.rs`, `get_publisher_link_stats`, the migrations and
  `src/schema.sql`.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the ADR index and `AGENTS.md`.

## Constraints

- `two_way_playlist_link_count` counts the `publisher_to_playlist` rows with
  `is_confirmed_link` true (§3).
- Put the count on each row that gives `two_way_link_count`, with the same
  include and the same omission rule. Put it also on
  `GET /v1/feeds/{guid}` of a feed with the medium `publisher`, with no
  include. A feed of each other medium does not give the field (§3).
- Do not change `two_way_link_count`, `stated_rels`, `agreed_roles`,
  `confirmed_release_artists`, `first_named_count`, `release_track_counts`,
  `release_count`, `newest_release_at` or `GET /v1/publisher-links/stats`
  (§3).
- The artist list cache holds the new count with the other link facts. Keep
  the current rebuild rule of the cache.
- `confirmed_publisher` is an object with `feed_guid`, `title`, `role` and
  `role_agreement`. It is on each `musicL` row of `GET /v1/feeds/recent` and
  `GET /v1/feeds/by-name`, with no include. It is null when the playlist has
  no confirmed `music_to_publisher` row. A row of each other medium does not
  give the field (§5).
- Resolved point: §5 gives `both` or `one_side` for `role_agreement`. A
  confirmed link where no side states a role has `role_agreement` null
  (`role_agreement` in `src/query.rs`). In that case `confirmed_publisher`
  gives `role_agreement` null. The schema of the field is nullable.
- Add each new field to the matching entries of
  `tests/fixtures/v1-schema-fields.json`.
- Each changed comment names ADR 0102 and its section.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Write each comment and each document line in Simplified Technical English.
  Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

1. A router test: a publisher feed with one confirmed album and one confirmed
   playlist gives `two_way_link_count` 1 and `two_way_playlist_link_count` 1
   with `include=link_facts` on `GET /v1/feeds/recent`,
   `GET /v1/feeds/by-name`, `GET /v1/artist-names` and `GET /v1/search`
   (ADR 0102 acceptance 4).
2. The same feed: `GET /v1/feeds/{guid}` with no include gives
   `two_way_playlist_link_count` 1 (ADR 0102 acceptance 4).
3. The same feed: `stated_rels`, `agreed_roles`, `confirmed_release_artists`
   and `release_count` give the values of the album only.
4. A router test: a playlist with a confirmed link gives `confirmed_publisher`
   with the GUID, the title and the role of the publisher feed on
   `GET /v1/feeds/recent?medium=musicL` and on `GET /v1/feeds/by-name`. A
   playlist with a one-way link gives null (ADR 0102 acceptance 5).
5. A router test: an album row of `GET /v1/feeds/recent` does not give
   `confirmed_publisher`.
6. `tests/fixtures/v1-schema-fields.json` names `two_way_playlist_link_count`
   and `confirmed_publisher`, and `tests/adr0044_field_removal_tests.rs`
   passes (ADR 0102 acceptance 8, for these fields).
7. Each existing test passes with no change of its assertions.

## Checks

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files and the result of each check. Name each point where the
code did not let you follow this packet.
