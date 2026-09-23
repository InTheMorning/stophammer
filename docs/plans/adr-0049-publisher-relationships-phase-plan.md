# ADR 0049 Phase Plan: Publisher Relationships Are RSS Facts

Owner: [ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md), Accepted
2026-09-23.

The client document is
[the publisher relationship request](v4vmm-publisher-relationship-request.md).
The [review checklist](../reviews/adr-0049-review-checklist.md) applies after
each task.

## Goal

The API reports each publisher relationship as a set of RSS facts. Each derived
value has a name or a companion field that gives its derivation. No code path
uses the host or the URL layout of a feed.

## Non-Goals

- Conditional GET for a feed fetch. A separate ADR decides it.
- A publisher link in the `import` mode or the `ndjson` mode. The `refresh` pass
  covers the feeds that these modes add.
- A change to the `track_artist` rule. It stays: the item author, then the feed
  artist name. A track with no item author thus gets a different
  `track_artist` when the feed `release_artist` changes. A
  `track_artist_source` field is not in this plan.
- A change to how an artist credit is made. Only the text that goes into the
  feed credit changes.
- A listing route for publisher feeds. ADR 0038 Decision B still keeps
  publisher feeds out of the public lists.
- A split of "feat." credits in the artist count.

## Current State

Commit `d5bd2c1` and the local database dated 2026-04-19 give these facts.

- The node matches a back-link by `feedGuid` only. This occurs in
  `has_reciprocal_music_remote_item` in `src/api.rs`, and in
  `load_publisher` and `load_track_publisher` in `src/query.rs`.
- Ingest sets `publisher` to "Wavlake" for a Wavlake album. For a different
  album, it uses the publisher feed title when the back-link matches, and the
  `itunes:owner` name if not.
- Ingest copies the publisher feed title into `release_artist` for a Wavlake
  album. It uses `wavlake_artist_name_from_links` before the owner name.
- The ingest of a publisher feed repairs the linked music feeds through
  `repair_linked_music_feeds_after_publisher_ingest`.
- The parser does not read `rel`. `IngestRemoteFeedRef` has 4 fields in each of
  the two crates.
- `upsert_feed` in `src/db.rs` sets `feed_url = excluded.feed_url`. The column is
  `NOT NULL UNIQUE`.
- The node logs `source_url` and does not store it.
- 44 of the 1,642 publisher feeds that albums name are indexed. None of the 49
  indexed publisher feeds is on Wavlake. Thus nobody knows yet if the verifier
  chain accepts a Wavlake artist feed.
- The last migration is `0034_feed_last_build_date.sql`.

## Decisions This Plan Makes

The ADR leaves these points to the plan. A task packet does not change them.

1. **An observation changes only when the GUID changes.** A new URL, or a known
   URL that gives a different GUID, writes a row and emits an event. The same
   URL with the same GUID writes nothing. `observed_at` is thus the time when
   the node first saw that URL give that GUID. Without this rule, each ingest
   emits an event.
2. **The observations have their own event.** The new event type is
   `FeedUrlObserved`. `FeedUpserted` does not change for observations.
3. **The migration seeds the observations.** Each stored `feed_url` becomes an
   observation with `observed_at = feeds.created_at`. A community node runs the
   same migration on the same replicated rows, so the result is the same.
4. **Ingest keeps the stored URL.** When the GUID is already indexed, ingest
   writes the stored `feed_url` into the `Feed` that it signs. `upsert_feed`
   does not change. A community node then applies the same value.
5. **The resolver is one function.** `db::resolve_listed_feed` gives `guid`,
   `feed_url` or `unresolved`. The publisher views, the artist count, the
   `publisher_feed_title` field and the statistics route all call it.
6. **The artist count reads stored values.** It counts only the feeds with
   `release_artist_source = 'itunes_author'`. A feed that has not had an ingest
   since the migration has a null source, and the count omits it.
7. **The contract document marks `rel` as non-standard.** The field
   descriptions in the `ToSchema` doc comments and in `docs/API.md` say so. The
   response carries no extra marker field.
8. **Two `rel` values are equal after trim and ASCII lowercase.** `role` is
   that normalized value. `publisher_rel` and `music_rel` stay raw.
9. **A new route gives the link counts.** `GET /v1/publisher-links/stats` gives
   the number of listed links for each resolution. The `refresh` pass reads it.
10. **The crawler follows links in the batch path and the gossip path.** The
    batch path serves the `feed` mode and the `refresh` mode.
11. **The resolved side replaces the declared GUID in two fields only.**
    `publisher_feed_guid` and `music_feed_guid` give the resolved GUID, and the
    declared value when a side does not resolve. `remote_feed_guid` always
    gives the declared value.
12. **A remote item with no `medium` follows the Podcast Namespace.** The
    default is `podcast`, so the item is not a listed album. The node keeps
    refusing a publisher feed with no `medium="music"` item. The Jimmy V
    publisher feed is such a feed. The operator decided this on 2026-09-23.
13. **A `rel` with a comma is one value.** `"artist, producer"` stays one
    string in `role`. A client can split it. The operator decided this on
    2026-09-23.

## Affected Modules

| Repository | Module | Change |
|---|---|---|
| `stophammer-parser` | `src/types.rs`, `src/engine.rs` | `rel` on `IngestRemoteFeedRef` |
| `stophammer` | `src/ingest.rs`, `src/model.rs`, `src/db.rs`, `src/schema.sql`, `migrations/` | `rel` columns, URL observations, `release_artist_source` |
| `stophammer` | `src/event.rs`, `src/apply.rs` | `FeedUrlObserved` event |
| `stophammer` | `src/api.rs` | ingest text fields, stable `feed_url`, deletion of the Wavlake rules |
| `stophammer` | `src/query.rs`, `src/openapi.rs` | publisher view, feed fields, statistics route |
| `stophammer-crawler` | `src/modes/batch.rs`, `src/modes/gossip.rs`, `src/modes/refresh.rs`, new `src/follow.rs` | link following, the count report |
| `stophammer` | `tests/`, `docs/` | real-feed fixtures, reference documents |

## Sequence

Each task is one commit in one repository. Each task ends green.

| Task | Repository | Result | Needs |
|---|---|---|---|
| [001](../tasks/adr-0049-task-001-parser-reads-rel.md) | `stophammer-parser` | The parser reads `rel` | none |
| [002](../tasks/adr-0049-task-002-real-feed-fixtures.md) | `stophammer` | Real-feed fixtures, and a probe that the verifier chain accepts a Wavlake artist feed | 001 |
| [003](../tasks/adr-0049-task-003-node-stores-rel.md) | `stophammer` | The node stores `rel`. Migration 0035 | 001 |
| [004](../tasks/adr-0049-task-004-url-observations.md) | `stophammer` | URL observations, `FeedUrlObserved`, stable `feed_url`. Migration 0036 | 002, 003 |
| [005](../tasks/adr-0049-task-005-back-link-resolver.md) | `stophammer` | The resolver and the relationship fields | 004 |
| [006](../tasks/adr-0049-task-006-role-fields.md) | `stophammer` | `publisher_rel`, `music_rel`, `role`, `role_source` | 003, 005 |
| [007](../tasks/adr-0049-task-007-text-fields.md) | `stophammer` | Text fields from one source each. The Wavlake rules are deleted. Migration 0037 | 005 |
| [008](../tasks/adr-0049-task-008-artist-count.md) | `stophammer` | `distinct_release_artist_count` and `distinct_release_artists` | 005, 007 |
| [009](../tasks/adr-0049-task-009-link-stats-route.md) | `stophammer` | `GET /v1/publisher-links/stats` | 005 |
| [010](../tasks/adr-0049-task-010-batch-follows-links.md) | `stophammer-crawler` | The batch path follows publisher links | 004 deployed |
| [011](../tasks/adr-0049-task-011-refresh-reports-unresolved.md) | `stophammer-crawler` | The `refresh` pass reports the unresolved count | 009, 010 |
| [012](../tasks/adr-0049-task-012-gossip-follows-links.md) | `stophammer-crawler` | The gossip path follows publisher links | 010 |
| [013](../tasks/adr-0049-task-013-reference-documents.md) | `stophammer` | Reference documents and `AGENTS.md` | 002 to 012 |

Task 001 starts first. After it, task 002 and task 003 can go in parallel. The
fixtures come from the parser, so task 002 waits for task 001. Task 004 uses the
fixtures of task 002.

The migrations merge in number order: task 003, then task 004, then task 007.
A migration version is its position in the `MIGRATIONS` array (ADR 0046). If a
task merges out of order, it takes the next free number and the report says so.

## Schema And API Implications

Storage:

- Migration 0035 adds a nullable `rel` column to `feed_remote_items_raw` and
  to `track_remote_items_raw`.
- Migration 0036 adds `feed_url_observations` and seeds it from `feeds`.
- Migration 0037 adds a nullable `release_artist_source` column to `feeds`.
- ADR 0046 applies to each migration. If a test database already records the
  version, the task adds an `ensure_<name>_schema` repair.

Events:

- `FeedRemoteItemRaw` and `TrackRemoteItemRaw` gain `rel` with
  `#[serde(default)]`. An earlier event still decodes.
- `Feed` gains `release_artist_source` with `#[serde(default)]`.
- `FeedUrlObserved` is a new event type. Deploy each community node before the
  primary node sends it.

API, all additive inside `v1`:

- The `publisher` view: `music_names_publisher`, `publisher_lists_music`,
  `publisher_link_resolution`, `publisher_link_observed_at`, `publisher_rel`,
  `music_rel`, `role`, `role_source`.
- A feed read: `release_artist_source`, `publisher_feed_title`. A publisher
  feed read also gives `distinct_release_artist_count` and
  `distinct_release_artists`.
- A track read: `release_artist_source`, when the track read gives
  `release_artist`.
- A new route: `GET /v1/publisher-links/stats`.

These values change for clients:

- `reciprocal_declared` and `two_way_validated` become true for a Wavlake album
  when the node resolves the link through a `feed_url` observation.
- `publisher_text` becomes the `itunes:owner` name for each album after its next
  ingest.
- `release_artist` no longer comes from the publisher feed title or from a
  Wavlake link path.

## Risk Areas

- **The verifier chain can refuse a Wavlake artist feed.** No such feed is
  indexed. Task 002 ingests one in a test. If the chain refuses it, stop the
  plan and report to the operator.
- **A feed that moved keeps its old URL.** Decision 4 keeps the first stored
  URL. After a permanent redirect, the node still stores the old URL, and the
  `refresh` pass fetches it. The redirect gives the new body, so ingest works.
  If the old URL stops, the feed is lost to the pass. The observation table
  holds the new URL. A later decision can let the pass use it.
- **The event order on a community node.** A primary node that sends
  `FeedUrlObserved` to an old community node causes an error on that node.
  Deploy each community node first.
- **Old stored values.** Each stored `publisher` and `release_artist` keeps the
  old rule until the next ingest. The operator runs `refresh --force` after
  task 010 is deployed.
- **The first pass takes a long time.** It fetches about 8,300 feeds, and the
  Wavlake album fetches alone take about 2 hours 46 minutes at the default host
  delay.
- **The gossip mode has no host throttle.** It limits only the number of fetches
  at one time. Task 012 adds a host throttle for follow fetches only, so that a
  podping for one artist feed does not send 131 requests to one host.
- **The cost of the resolver.** A publisher feed can list 131 albums. Each row
  in the publisher view resolves each listed item with two indexed lookups. Task
  005 reports the read time of the `detox-artist` fixture. Task 009 reports the
  time of the statistics route against a copy of the local database.
- **The URL branch of the artist count scans a table.** Task 008 matches each
  observed URL of a publisher with `feed_remote_items_raw.remote_feed_url`. No
  index covers that column. On 2026-09-23 the local database held 7,512 rows,
  and one scan took 2.6 ms. A publisher read does one scan for each URL that
  gave its GUID, usually one. If the table becomes ten times larger, add an
  index in a migration.

## Test Strategy

- Each task keeps the current suite green. A task deletes a test only when that
  test states the Wavlake rule that ADR 0049 supersedes. The task report names
  each deleted test.
- Task 002 adds the real-feed fixtures. Tasks 005 to 009 test against them.
- Each migration has a test in `tests/migration_tests.rs`.
- Each new event has a round-trip test in `tests/event_tests.rs` and an apply
  test.
- Task 005 adds a guard that `src/query.rs` holds no `remote_feed_guid ==`.
  Task 007 adds the same guard for `src/api.rs`, after it deletes the last
  match there. Only `db::resolve_listed_feed` compares a listed GUID with a
  feed GUID. The time it broke: three separate matches by
  `feedGuid` hid the Wavlake defect.
- Task 007 adds a guard that `src/api.rs` holds no `fn is_wavlake_url` and no
  `fn wavlake_artist_name_from_links`. The time it broke: the host rule of
  commits `984b78c` and `8dc3789`. The guard names the deleted functions, not
  the host literal, because `classify_platform_url` keeps `wavlake.com` for the
  source platform claims.

Visual checks, kept apart:

- After task 013, load `/api` on the node and confirm that the new fields and
  the new route show their examples.

## Rollback

Each task is one commit and reverts alone. These tasks need more:

- Task 004. A revert leaves `feed_url_observations` and the events in the log.
  A node that goes back to the old code cannot decode a `FeedUrlObserved`
  event. Revert the primary node first. Revert a community node only after it
  applies each event that it receives.
- Task 007. A revert brings back the Wavlake rules. The values that ingest
  wrote before the revert stay until the next ingest.
- Task 010. A revert stops the follow step. The publisher feeds that the
  crawler already submitted stay in the index.
- Each migration is additive. A revert of the code does not remove a column.

## Open Questions

None. An escalation from a task comes back to the planning model.
