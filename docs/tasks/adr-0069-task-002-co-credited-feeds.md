# ADR 0069 Task 002: A Publisher Read Gives The Feeds That Share Its Albums

Owner: [ADR 0069](../adr/0069-an-album-confirms-each-credit.md) section 4.
Release 0.6.0. Task 001 comes first.

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/feeds/{guid}?include=publisher` on a publisher feed gives
`co_credited_feeds`: each other publisher feed that a confirmed album of this
feed also confirms.

## Files To Inspect

- `src/query.rs`: `load_publisher`, `confirmed_and_unconfirmed_release_artists`,
  `link_facts`, `FeedResponse`, the feed read handler
- `src/openapi.rs`: the `FeedResponse` schema
- `tests/adr0061_*` and `tests/adr0068_link_facts_tests.rs`: the fixtures

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0069_co_credited_feeds_tests.rs`, new
- `docs/API.md`, `docs/publisher-links-guide.md`

## Constraints

- **Value.** For each `publisher_to_music` row of this feed with
  `two_way_validated` true, read the album's own `publisher` rows. Each other
  publisher feed on a `music_to_publisher` row of that album with
  `two_way_validated` true is an entry. Group by its feed GUID.
- **Entry.** `feed_guid`, `title`, `roles` (the different raw `rel` values
  that the album items give that feed, sorted, empty when none), and
  `album_count` (the number of shared confirmed albums).
- **Sequence.** `album_count` descending, then `feed_guid`.
- **Scope.** Only a feed with the medium `publisher`, and only with
  `include=publisher`. Another feed gives no field. The value is computed at
  read time, and nothing is stored (ADR 0069 §4).
- **Cost.** Measure the read of the publisher feed with the most links on a
  copy of the production data, before and after. Report both times.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0069_co_credited_feeds_tests.rs`.

- A label with one confirmed album that credits an artist, who lists the
  album: the label read gives the artist with `roles` `["artist"]` and
  `album_count` 1. The artist read gives the label with `roles` `["label"]`.
- An artist feed that lists the album, when the album does not credit it, is
  not in the label's list.
- An artist that the album credits, when the artist feed does not list the
  album, is not in the list.
- Two shared albums give `album_count` 2.
- A music feed read gives no `co_credited_feeds`.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0069_co_credited_feeds_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results, and the two read times
3. behavior changed
4. deviations from this task
5. unresolved concerns
