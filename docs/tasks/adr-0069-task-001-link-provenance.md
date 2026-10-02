# ADR 0069 Task 001: Each Link Says How The Album Names It

Owner: [ADR 0069](../adr/0069-an-album-confirms-each-credit.md) sections 1
and 3. Release 0.6.0.

Repositories: `stophammer-parser`, then `stophammer`. The crawler passes the
parser type through, so it changes only by its dependency. The operator
commits.

## Goal

The parser marks the feed-level `remoteItem` that is the publisher of the
album. The node stores the mark in `source`, and each row of the `publisher`
view gives `album_names_as`: `publisher`, `credit` or null.

## Files To Inspect

- `stophammer-parser/src/engine.rs`: `extract_feed_remote_items`,
  `append_remote_ref`
- `stophammer-parser/src/types.rs`: `IngestRemoteFeedRef`
- `src/ingest.rs`: `IngestRemoteFeedRef`
- `src/api.rs`: the `feed_remote_items` mapping that sets
  `source: "podcast_remote_item"` (two places)
- `src/query.rs`: `PublisherResponse`, `build_publisher_row`,
  `music_to_publisher_facts`, `publisher_to_music_facts`, `load_publisher`
- `src/openapi.rs`: the schema of the publisher row
- `tests/adr0068_link_facts_tests.rs`: the fixtures

## Files Likely To Change

- `stophammer-parser/src/types.rs`, `stophammer-parser/src/engine.rs`
- `src/ingest.rs`, `src/api.rs`, `src/query.rs`
- `tests/adr0069_link_provenance_tests.rs`, new
- `docs/API.md`: the `publisher` view

## Do Not Touch

- The two-way checks of ADR 0049 §3 and §4.
- `feed_remote_items_raw` has its `source` column. Add no migration.
- The event types. `source` is already a field of the signed remote item.
- Item-level remote items (ADR 0038).

## Constraints

- **Parser.** `IngestRemoteFeedRef` gets `publisher_reference: bool`, with
  `#[serde(default)]`. It is `true` for each item inside
  `<podcast:publisher>`. When the channel has no `<podcast:publisher>`, it is
  `true` for the first bare `remoteItem` with `medium="publisher"`, and
  `false` for each other item. The order of the elements in the channel does
  not change the result when `<podcast:publisher>` exists.
- **Node ingest.** `IngestRemoteFeedRef` in `src/ingest.rs` gets the same
  field with `#[serde(default)]`. An older crawler sends no field, and each
  item stays `podcast_remote_item`. The mapping in `src/api.rs` sets `source`
  to `podcast_publisher` when the field is `true`.
- **Response.** `PublisherResponse` gets `album_names_as: Option<String>`.
  On a `music_to_publisher` row, the value comes from the album's own item.
  On a `publisher_to_music` row, it comes from the album item that names this
  publisher feed, and it is null when `music_names_publisher` is false.
  `podcast_publisher` gives `publisher`, and `podcast_remote_item` with
  `medium="publisher"` gives `credit`.
- **Old rows.** A row that an older ingest stored gives `credit` for each
  item, also for its publisher. The deploy step below corrects them.

## Deploy Step

After the deploy, the operator runs one `refresh` pass with `--force`, so
each record sends its body again and stores the new `source` values. With the
fix of release 0.5.0, the pass signs only the events of the changed remote
items.

## Acceptance Criteria

Mechanical. Each is a test.

Parser, in `stophammer-parser`:

- An album with `<podcast:publisher>` and one bare `medium="publisher"` item
  gives `publisher_reference` `true` for the nested item and `false` for the
  bare one. It does the same when the bare item comes first in the channel.
- An album with two bare `medium="publisher"` items and no
  `<podcast:publisher>` gives `true` for the first only.
- A publisher feed's `medium="music"` items give `false`.

Node, in `tests/adr0069_link_provenance_tests.rs`:

- An album that names a label as publisher and an artist as a credit, each
  listing the album back, gives two two-way rows. `album_names_as` is
  `publisher` for the label and `credit` for the artist.
- The label read and the artist read give the same values on their rows.
- A publisher feed that lists the album, when the album does not name it,
  gives `album_names_as` null.
- An ingest payload with no `publisher_reference` field gives `credit`.
- The ADR 0044 guards pass with the new field in the schema.

## Test Commands

```bash
cd stophammer-parser && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd .. && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check
cd stophammer-crawler && cargo build && cargo test
```

## Expected Final Report

1. files changed, in each repository
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
