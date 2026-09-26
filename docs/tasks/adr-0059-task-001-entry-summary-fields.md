# ADR 0059 Task 001: Entry Summary Fields

Owner: [ADR 0059](../adr/0059-an-entry-that-names-a-feed-gives-its-summary.md),
Accepted on 2026-09-25. Plan:
[client requests work plan](../plans/client-requests-work-plan.md), item 5.

Repository: `stophammer`.

## Goal

Each `publisher` entry and each `remote_items` entry gives the title, the
image and the artist of the feed that it names. This applies to a feed read
and to a track read.

## Files To Inspect

- `docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md`, all of it
- `src/query.rs`: `PublisherResponse`, `FeedRemoteItemResponse`,
  `TrackRemoteItemResponse`, `feed_remote_item_response`,
  `load_track_remote_items`, `build_publisher_row`,
  `publisher_to_music_facts`, `music_to_publisher_facts`,
  `resolved_or_declared`, `load_publisher`, `load_track_publisher`,
  `web_url_or_none`
- `src/db.rs`: `resolve_listed_feed`, `ListedFeedResolution`
- `src/openapi.rs`: the examples of the feed and track reads
- `docs/API.md`: the `publisher` and `remote_items` includes
- A test file that ingests a publisher feed and its albums, for example
  `tests/adr0049_*.rs` or `tests/client_requests_search_fields_tests.rs`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/adr0059_entry_summary_tests.rs`, new

## Do Not Touch

- The resolution order of `resolve_listed_feed`, and each existing field of
  the two views
- The storage. This task adds no migration and no stored value
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The fields.** Add these four fields to `PublisherResponse`,
  `FeedRemoteItemResponse` and `TrackRemoteItemResponse`:

  | Field | Value |
  |---|---|
  | `remote_feed_title` | `feeds.title` of the named feed |
  | `remote_feed_image_url` | `feeds.image_url` of the named feed, through `web_url_or_none` |
  | `remote_release_artist` | `feeds.release_artist` of the named feed |
  | `remote_release_artist_source` | `feeds.release_artist_source` of the named feed |

  Each key is always in the response. Its value is null when the node holds
  no feed for the entry. Do not use `skip_serializing_if` on these fields.
- **The named feed.**
  - A `publisher` entry names the feed on the other side: the album on a row
    of the direction `publisher_to_music`, and the publisher on a row of the
    direction `music_to_publisher`. Use the `ListedFeedResolution` that the
    row has. Do not resolve a second time. An `Unresolved` row gives
    null in each field.
  - A `remote_items` entry resolves its feed with `resolve_listed_feed`, from
    the stored `remote_feed_guid` and `remote_feed_url`. Use the raw stored
    URL for the resolution, not the value after `web_url_or_none`.
- **The cost.** Read the four values with one point read by `feed_guid` for
  each entry, in one helper, for example `feed_summary(conn, feed_guid)`. Do
  not read the full `Feed` row with `db::get_feed` when a smaller query is
  sufficient.
- The image is the channel image of the named feed, not a resolved image
  (ADR 0059 section 3).
- Add the fields to the examples in `src/openapi.rs` and to `docs/API.md`.
  Write in ASD-STE100 Simplified Technical English. Run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics docs/API.md`.
  Correct each finding in your own lines.

## Implementation Steps

1. Add the helper that reads the summary of one feed.
2. Add the fields to the three response types, and fill them.
3. Update the examples and `docs/API.md`.
4. Add the tests below.
5. Add the timing test below, and report its numbers.
6. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0059_entry_summary_tests.rs`. Each failure
message names ADR 0059 and its guard:

- A publisher read with `include=publisher` gives the title, the image and
  the artist of each indexed album.
- An album read with `include=publisher` gives the summary of the publisher.
- A `remote_items` entry for an indexed feed gives its summary. An entry for a
  feed that is not indexed gives null in each of the four fields, and each
  key is in the response.
- An entry can have a `feedGuid` that is not indexed and a `feedUrl` of an
  indexed feed. That entry gives the summary of the feed at that URL.
- A named feed with a `javascript:` image gives a null
  `remote_feed_image_url`.
- A track read with `include=remote_items` gives the summary of each indexed
  named feed.
- The gate is green.

Measurement, reported and not asserted:

- A test ingests one publisher feed that lists 150 indexed albums. It reads
  the publisher with `include=publisher` 20 times, and prints the median
  time. Run it before the change and after the change, with
  `cargo test --release --test adr0059_entry_summary_tests -- --nocapture`.
  Report both numbers. Mark the test `#[ignore]`, so that the gate does not
  run it.

## Test Commands

```bash
cargo build
cargo test --test adr0059_entry_summary_tests
cargo test --release --test adr0059_entry_summary_tests -- --ignored --nocapture
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results, with the two timing numbers
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- A `publisher` row has no resolution in hand, and a second resolution is
  necessary.
- The timing after the change is more than two times the timing before it.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0059-task-001-entry-summary-fields.md`, all of it
- `docs/adr/0059-an-entry-that-names-a-feed-gives-its-summary.md`
- The files in "Files To Inspect"

Goal:
- Each `publisher` and `remote_items` entry gives the summary of the feed
  that it names.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Run `cargo clippy --all-targets -- -D warnings`, not a smaller target set.
  Report its real result.

Do not touch:
- The resolution order, the existing fields and the storage
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria", and the two timing numbers.
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
