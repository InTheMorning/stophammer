# ADR 0049 Task 004b: The Stored Feed URL Resolves On Each Node

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) §1 and
§3.
Plan: [phase plan](../plans/adr-0049-publisher-relationships-phase-plan.md),
decision 3.
Needs: tasks 004, 005, 008 and 009 merged.

## Why This Task Exists

Migration 0036 copies each stored `feeds.feed_url` into
`feed_url_observations`. The copy is not an event. On 2026-09-23 a new
community node showed the result: 1,272 feeds and 0 observations. A new node
starts empty, so the migration copies nothing. The node then receives the feeds
as `FeedUpserted` events, and that event records no observation.

So the primary node can resolve a link as `feed_url` where a new community node
gives `unresolved`. The two nodes must give the same answer.

A fix in `apply` is not sufficient. The event log also holds earlier
`FeedUpserted` events with earlier URLs of a feed. A new node that records each
of them would hold URLs that the primary node does not hold.

## Goal

The resolver also accepts the stored `feeds.feed_url` of an indexed feed. That
column is replicated, so each node gives the same result.

## Files To Inspect

- `src/db.rs`: `resolve_listed_feed`, `ListedFeedResolution`,
  `get_feed_url_observation`, `get_publisher_album_release_artists`,
  `get_publisher_link_stats`
- `src/apply.rs`: the `FeedUpserted` and `FeedUrlObserved` arms
- `tests/adr0049_resolver_tests.rs`, `tests/adr0049_artist_count_tests.rs`,
  `tests/adr0049_link_stats_tests.rs`, `tests/adr0049_url_observation_tests.rs`
- `docs/plans/adr-0049-publisher-relationships-phase-plan.md`, decision 3

## Files Likely To Change

- `src/db.rs`
- `tests/adr0049_replica_resolution_tests.rs`, new

## Do Not Touch

- `src/apply.rs`. The fix is in the resolver, not in the event application
- migration 0036 and each other merged migration
- the record rule and the ingest path of task 004
- the publisher-view rules of tasks 005 and 006
- `stophammer-crawler`, `stophammer-parser`

## Constraints

The resolver order becomes:

1. The listed `feedGuid` is indexed: `Guid`.
2. An observation for the listed `feedUrl` names an indexed GUID: `FeedUrl`,
   with the `observed_at` of the observation.
3. An indexed feed has `feeds.feed_url` equal to the listed `feedUrl`:
   `FeedUrl`, with `observed_at = feeds.created_at`.
4. Else: `Unresolved`.

- Step 3 is an exact string comparison. It uses the `UNIQUE` index on
  `feeds.feed_url`. It does not parse or change the URL.
- Step 3 gives the same `observed_at` as the migration copy, because migration
  0036 wrote `created_at`. So on the primary node, step 2 and step 3 give the
  same output for a copied row.
- Step 2 stays before step 3. A `FeedUrlObserved` event can move a URL to a
  different GUID, and each node applies that event.
- The artist count reads the URLs that name publisher P from two sources: the
  observations with `feed_guid = P`, and `feeds.feed_url` of P. Remove
  duplicates. Then match them as today, and confirm each candidate with
  `resolve_listed_feed`.
- The statistics route uses `resolve_listed_feed`, so it needs no change.
- Update the doc comment of `resolve_listed_feed` to give the four steps and
  the reason for step 3.

## Implementation Steps

1. Add step 3 to `resolve_listed_feed`, with a unit test.
2. Add P's `feed_url` to the URL list in `get_publisher_album_release_artists`.
3. Add the replica test below.
4. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A unit test: a feed with `feed_url` U and no observation of U resolves a
  listed item with that URL as `FeedUrl`, with `observed_at` equal to the
  feed's `created_at`.
- A unit test: an observation of U that names GUID Y wins over a feed X with
  `feed_url` U, when Y is indexed.
- **Replica test.** Database A ingests `detox-artist`, then `detox-album` with
  `canonical_url` equal to the `feedUrl` that the artist feed lists. Database B
  applies each event of A except each `FeedUrlObserved` event. B then holds no
  observation, as a new community node does for a feed from before task 004.
  For both databases, compare these values:
  - the `publisher` view of the album,
  - the `publisher` view of the artist feed,
  - the artist count of the artist feed,
  - the statistics counts.

  All four must be equal, and the album row must give
  `publisher_link_resolution = "feed_url"`.
- The existing ADR 0049 tests pass with no edit.

## Test Commands

```bash
cargo build
cargo test --test adr0049_replica_resolution_tests
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

- an existing test needs a changed expected value
- the replica test shows a difference that step 3 does not remove
- the events of A cannot be applied to B without a change to `src/apply.rs`

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0049-task-004b-stored-url-resolves.md`, in full
- `src/db.rs`: `resolve_listed_feed`, `get_publisher_album_release_artists`,
  `get_publisher_link_stats`
- `src/apply.rs`: how events are applied
- `tests/adr0049_url_observation_tests.rs`: how a test applies the events of one
  database to a second one

Goal:
- The resolver also accepts the stored `feeds.feed_url` of an indexed feed, so
  a new community node resolves links as the primary node does.

Constraints:
- Resolver order: indexed GUID; observation of the URL; exact match of
  `feeds.feed_url` with `observed_at = feeds.created_at`; else unresolved.
- Step 3 is an exact comparison on the `UNIQUE` column. No URL parsing.
- The artist count adds P's own `feed_url` to the URLs that it matches, with
  duplicates removed.
- Update the doc comment of `resolve_listed_feed`.

Do not touch:
- `src/apply.rs`, merged migrations, the task 004 ingest path, the task 005
  and 006 view rules
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Unit tests prove:
  - A stored `feed_url` with no observation resolves as `FeedUrl`, with the
    feed's `created_at`.
  - An observation wins over a stored `feed_url`.
- The replica test in the task file passes. The publisher views, the artist
  count and the statistics are equal on A and B.
- The existing ADR 0049 tests pass unedited.

Test commands:
- `cargo build`
- `cargo test --test adr0049_replica_resolution_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
