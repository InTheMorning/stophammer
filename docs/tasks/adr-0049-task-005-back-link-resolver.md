# ADR 0049 Task 005: The Back-Link Resolver And The Relationship Fields

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §3 and
§4.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 5.
Needs: task 004.

## Goal

One function resolves a listed feed. The `publisher` view on a feed read and on
a track read uses it, and reports each relationship fact.

## Files To Inspect

- `src/query.rs`: `PublisherResponse`, `load_publisher`,
  `load_track_publisher`, `publisher_direction`, `expected_reciprocal_medium`
- `src/db.rs`: `get_feed`, `get_feed_remote_items_for_feed`, the track
  remote-item readers, and `get_feed_url_observation` from task 004
- `src/openapi.rs`: the `include=publisher` examples
- `tests/adr0038_tests.rs` and `tests/api_canonical_query_tests.rs`: the tests
  that assert `reciprocal_declared` or `two_way_validated`
- the task 002 fixtures

## Files Likely To Change

- `src/db.rs`: the resolver
- `src/query.rs`: the two publisher views
- `src/openapi.rs`: the examples
- `tests/adr0049_resolver_tests.rs`, new
- the existing tests whose expected values this task changes

## Do Not Touch

- `src/api.rs`. Its GUID match in `has_reciprocal_music_remote_item` goes in
  task 007
- the role fields. Task 006 adds them
- the storage and the events

## Constraints

The resolver:

```rust
pub enum ListedFeedResolution {
    Guid { feed_guid: String },
    FeedUrl { feed_guid: String, observed_at: i64 },
    Unresolved,
}

pub fn resolve_listed_feed(
    conn: &Connection,
    listed_feed_guid: &str,
    listed_feed_url: Option<&str>,
) -> Result<ListedFeedResolution, DbError>
```

1. If `feeds` holds `listed_feed_guid`, give `Guid`.
2. If an observation for `listed_feed_url` names a GUID that `feeds` holds, give
   `FeedUrl`.
3. Else give `Unresolved`.

The resolver never parses a URL, never compares URL parts and never guesses.

The rows of the feed view, for feed F and each remote item I of F:

- **I names a publisher** (`music_to_publisher`). `music_names_publisher` is
  true. Resolve I to publisher P. If P resolves, look at each `medium="music"`
  item of P and resolve it. The first one that resolves to F gives
  `publisher_lists_music = true` and its resolution. If none does, or P does
  not resolve, `publisher_lists_music` is false and the resolution is
  `unresolved`.
- **I lists an album** (`publisher_to_music`). `publisher_lists_music` is true.
  The resolution is the resolution of I. If the album A resolves, look at each
  publisher item of A. `music_names_publisher` is true when one of them
  resolves to F.

For both directions:

- `publisher_link_resolution` is `"guid"`, `"feed_url"` or `"unresolved"`.
- `publisher_link_observed_at` is `observed_at` for `feed_url`, and `null` for
  the other two.
- `reciprocal_declared` and `two_way_validated` are both
  `music_names_publisher && publisher_lists_music`.
- `reciprocal_medium` is the medium of the matching item on the other side, or
  `null`.
- `remote_feed_guid` and `remote_feed_url` stay the declared values of I.
- `publisher_feed_guid` and `music_feed_guid` give the resolved GUID of that
  side. When a side does not resolve, they give the declared value. The same
  rule applies to `publisher_feed_url` and `music_feed_url`.

The track view uses the same rules. The track side is the feed of the track.

Add the four new fields to `PublisherResponse`. Do not rename or remove a field.

## Implementation Steps

1. Add the enum and `resolve_listed_feed` to `src/db.rs`, with unit tests for
   each branch.
2. Rewrite the reciprocal part of `load_publisher` with the rules above.
3. Do the same for `load_track_publisher`.
4. Add the fields and the OpenAPI examples.
5. Update each existing test whose expected value changes. Name each one in the
   report with the old and the new value.
6. Add a guard test: `src/query.rs` holds no `remote_feed_guid ==`.
7. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- Unit tests prove the three resolver branches, and that an observation that
  names a GUID not in `feeds` gives `Unresolved`.
- DETOX test: ingest `detox-artist`, then `detox-album` with `source_url` equal
  to the `feedUrl` that the artist feed lists. The album view gives
  `music_names_publisher = true`, `publisher_lists_music = true`,
  `publisher_link_resolution = "feed_url"`, and `two_way_validated = true`.
- DETOX test, other URL: ingest `detox-album` only at
  `https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a`. The album view
  gives `publisher_link_resolution = "unresolved"` and `two_way_validated =
  false`.
- Jimmy V test: ingest `jimmyv-publisher` and `jimmyv-produced-album`. The
  publisher view gives a row for that album with `publisher_lists_music = true`
  and `music_names_publisher = false`.
- RSS Blue test: `rssblue-publisher` and `rssblue-album` give
  `publisher_link_resolution = "guid"`.
- `no-publisher-album` gives no publisher row.
- The guard test passes.

## Test Commands

```bash
cargo build
cargo test --test adr0049_resolver_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi > /dev/null
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed, with each changed test and its old and new values
4. deviations from this task
5. unresolved concerns
6. the time of one feed read of `detox-artist` with the publisher include, from
   a release build

## Escalation Triggers

Stop and report when:

- an existing test asserts a value that the rules above change, and the test
  does not state a Wavlake or a GUID-match rule
- a fixture does not give the result that a criterion expects
- the track view has a shape that the rules above do not cover

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-005-back-link-resolver.md`, the section
  "Constraints", in full
- `src/query.rs`: `PublisherResponse`, `load_publisher`,
  `load_track_publisher`
- `src/db.rs`: `get_feed`, `get_feed_remote_items_for_feed`,
  `get_feed_url_observation`
- the fixtures under `tests/fixtures/adr0049/`

Goal:
- Add `db::resolve_listed_feed`, and make the feed and track `publisher` views
  report `music_names_publisher`, `publisher_lists_music`,
  `publisher_link_resolution` and `publisher_link_observed_at` through it.

Constraints:
- Use the enum, the signature, the three branches and the row rules exactly as
  the section "Constraints" gives them.
- The resolver never parses or compares URL parts.
- `reciprocal_declared` and `two_way_validated` are both
  `music_names_publisher && publisher_lists_music`.
- `remote_feed_guid` and `remote_feed_url` stay declared. The
  `publisher_feed_*` and `music_feed_*` fields give the resolved side, or the
  declared value when a side does not resolve.
- Add fields only. Rename or remove nothing.

Do not touch:
- `src/api.rs`
- role fields, storage, events

Acceptance criteria:
- The gate is green.
- Tests prove the resolver branches, including an observation whose GUID is not
  in `feeds`.
- The DETOX, other-URL, Jimmy V, RSS Blue and no-publisher cases give the
  values in the task file.
- A guard test proves that `src/query.rs` holds no `remote_feed_guid ==`.
- The report names each changed existing test with its old and new values.

Test commands:
- `cargo build`
- `cargo test --test adr0049_resolver_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
- `cargo run --bin gen_openapi > /dev/null`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
