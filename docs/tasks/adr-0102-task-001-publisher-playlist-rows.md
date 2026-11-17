# ADR 0102 Task 001: A Publisher Feed Gives Its Playlist Rows

Owner: [ADR 0102](../adr/0102-a-publisher-view-shows-its-playlists.md) §1, §2
and §4.

Repository: `stophammer`. This task is the first of three. Tasks 002 and 003
change the same file, `src/query.rs`, so they start only after this task is
complete and committed.

## Goal

On a publisher feed, `include=publisher` gives one `publisher_to_playlist` row
for each channel item with the medium `musicL`. On a `musicL` feed, the
`music_to_publisher` row of its publisher reference reads the `musicL` items
of the publisher feed for the back-link. The rows of an album do not change.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Medium helpers | reuse | `src/medium.rs` | `is_musicl` and `is_publisher` compare with the ASCII case ignored |
| Publisher row builder | extend | `src/query.rs` | `build_publisher_row` gets a third direction, and `publisher_direction` gives it |
| Back-link facts | extend | `src/query.rs` | `music_to_publisher_facts` selects the `musicL` items for a playlist |
| Forward-link facts | reuse | `src/query.rs` | `publisher_to_music_facts` resolves the listed feed and its publisher reference |
| Role functions | reuse | `src/query.rs` | `resolve_role`, `role_agreement` and `is_confirmed_link` give the role values of ADR 0049 §6 and §6a |
| Playlist router tests | extend | `tests/adr0101_playlist_publisher_tests.rs` | It ingests a publisher feed with `musicL` items |
| Link rule test helpers | reuse | `tests/adr0049_pr793_link_rules_tests.rs` | `ingest`, `names`, `lists`, `album`, `publisher` and `read` build linked feeds |
| List item test helpers | reuse | `tests/adr0060_list_items_tests.rs` | `ingest`, `entry` and `remote_items` build a `musicL` feed |

## Files To Inspect

- `docs/adr/0102-a-publisher-view-shows-its-playlists.md`: §1, §2 and §4
- `src/query.rs`:
  - `publisher_direction` (near line 3762)
  - `load_publisher` (near line 3645)
  - `build_publisher_row` (near line 3540). Each `match` on the direction.
  - `music_to_publisher_facts` (near line 3234). The filter
    `item.medium.as_deref() == Some("music")` near line 3253.
  - `publisher_to_music_facts` (near line 3297)
  - `named_feed_summary` (near line 3063)
  - `resolve_role`, `role_agreement`, `is_confirmed_link` (near line 3401)
  - `is_album_publisher_link`, `album_names_as_value`, `album_item_rank`
    (near line 3481)
  - Each filter on `"publisher_to_music"` or `"music_to_publisher"`: lines
    1773, 2276, 2325, 2377, 2381 and 3465. These must keep their current rows.
- `src/medium.rs`
- `tests/adr0101_playlist_publisher_tests.rs`
- `tests/adr0049_pr793_link_rules_tests.rs`: the helpers and the role tests
- `tests/adr0060_list_items_tests.rs`: the helpers

## Files Likely To Change

- `src/query.rs`
- `tests/adr0101_playlist_publisher_tests.rs`, or a new
  `tests/adr0102_playlist_links_tests.rs` for the new tests
- `src/openapi.rs` and `tests/fixtures/v1-schema-fields.json`, only if the
  `direction` field has an enum of values in the schema

## Do Not Touch

- `link_facts`, the list rows, the search rows, the artist list and its cache.
  Task 002 changes them.
- The route table and `GET /v1/node/capabilities`. Task 003 changes them.
- `src/db.rs`, the migrations and `src/schema.sql`. ADR 0102 stores no new
  data.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the ADR index and `AGENTS.md`.

## Constraints

- `publisher_direction` gives `publisher_to_playlist` for an item with the
  medium `musicL` only when the current feed has the medium `publisher`. Use
  `medium::is_musicl` and `medium::is_publisher`. A `musicL` item on a feed of
  another medium gives no row (§1).
- The `publisher_to_playlist` row uses `publisher_to_music_facts` and has each
  field of §1 with the values of §1. `publisher_lists_music` is true. The
  `match` arms of `build_publisher_row` treat `publisher_to_playlist` as
  `publisher_to_music`.
- In `music_to_publisher_facts`, the back-link filter selects the items with
  `medium::is_musicl` when the current feed is a `musicL` feed. For each other
  medium, keep the current filter `Some("music")` with no change (§2).
- The direction of the playlist row of §2 stays `music_to_publisher`.
  `reciprocal_medium` is `musicL` when the back-link matches.
- Resolved point: `reciprocal_medium` gives the stored medium of the matched
  item, as for an album. An item written as `MusicL` gives `MusicL`.
- Do not change a row of the directions `music_to_publisher` and
  `publisher_to_music` on an album (§4).
- Each changed comment names ADR 0102 and its section.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Write each comment and each document line in Simplified Technical English.
  Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a router test that sends ingests and reads through the
router.

1. A publisher feed lists one playlist with a `musicL` item, and the playlist
   names that publisher feed. `include=publisher` on the publisher feed gives
   one `publisher_to_playlist` row with `two_way_validated` true,
   `publisher_lists_music` true and `music_names_publisher` true (ADR 0102
   acceptance 1).
2. The same playlist: `include=publisher` on the playlist gives one
   `music_to_publisher` row with `two_way_validated` true and
   `reciprocal_medium` `musicL` (ADR 0102 acceptance 2).
3. An album that a publisher feed lists only with a `musicL` item gives a
   `music_to_publisher` row with `two_way_validated` false. A playlist that a
   publisher feed lists only with a `music` item gives `two_way_validated`
   false (ADR 0102 acceptance 3).
4. A `musicL` item with the medium value `MusicL` gives the same row as
   criterion 1.
5. A `musicL` feed that lists a `musicL` item gives no `publisher_to_playlist`
   row.
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
