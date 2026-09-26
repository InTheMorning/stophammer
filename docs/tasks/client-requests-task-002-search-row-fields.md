# Client Requests Task 002: Search Row Fields

Plan: [client requests work plan](../plans/client-requests-work-plan.md),
items 3 and 6. Owner of item 3: ADR 0042. Owner of item 6: ADR 0049
section 3.

Repository: `stophammer`.

## Goal

A search result gives the artist, the track count and the duration of its
row. A test proves that a publisher read gives `unresolved` for an album feed
that is deleted.

## Files To Inspect

- `docs/plans/client-requests-work-plan.md`, items 3 and 6
- `src/query.rs`: `SearchResponseItem`, `handle_search`, the code that fills
  each result, `music_to_publisher_facts`, `load_publisher`
- `src/openapi.rs`: the search route and its example
- `docs/API.md`: the search route
- `src/api.rs`: `handle_retire_feed` (`DELETE /v1/feeds/{guid}`)
- `docs/adr/0049-publisher-relationships-are-rss-facts.md`, section 3
- A test file that ingests a publisher feed and its albums, for example
  `tests/adr0049_*.rs`

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`, `docs/API.md`
- `tests/client_requests_search_fields_tests.rs`, new
- `docs/adr/0049-publisher-relationships-are-rss-facts.md`: one sentence in
  section 3 that names the new test

## Do Not Touch

- The search rank, the search filter, the cursor and the FTS index
- `src/api.rs` and `/node/info`. Another agent changes them at the same time
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The fields.** Add these fields to `SearchResponseItem`:

  | Result | Field | Stored value |
  |---|---|---|
  | feed | `release_artist` | `feeds.release_artist` |
  | feed | `release_artist_source` | `feeds.release_artist_source` |
  | feed | `episode_count` | `feeds.episode_count` |
  | track | `track_artist` | `tracks.track_artist` |
  | track | `duration_secs` | `tracks.duration_secs` |

  Use `#[serde(skip_serializing_if = "Option::is_none")]`, as the other
  summary fields do. Read the values in the same query that reads the other
  summary fields of the row. Do not add one query for each row.
- The schema of `SearchResponseItem` shows the fields. Add them to the search
  example in `src/openapi.rs` and to `docs/API.md`.
- **The deleted album.** A publisher feed lists an album. The album feed is
  indexed. The test deletes the album feed with `DELETE /v1/feeds/{guid}` and
  the admin token. The next read of the publisher with `include=publisher`
  gives that entry with `publisher_link_resolution` equal to `unresolved`.

  Put the test in the new test file. If the result is not `unresolved`, stop
  and report. Do not change the code.
- Add one sentence to ADR 0049 section 3 that names the test file and the
  test. Write it in ASD-STE100 Simplified Technical English. Run
  `python3 ~/.agents/skills/asd-ste100/scripts/ste_lint.py --check --no-heuristics`
  on the ADR and on `docs/API.md`. Correct each finding in your own lines.

## Implementation Steps

1. Add the fields and fill them in `handle_search`.
2. Update the schema example and `docs/API.md`.
3. Add the tests below.
4. Add the sentence to ADR 0049.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/client_requests_search_fields_tests.rs`:

- A search for an indexed feed gives a feed result with `release_artist`,
  `release_artist_source` and `episode_count` equal to the stored values. The
  failure message names musicindex request 3.
- A search for an indexed track gives a track result with `track_artist` and
  `duration_secs` equal to the stored values.
- After the delete of a listed album feed, the publisher read gives the entry
  as `unresolved`. The failure message names ADR 0049 section 3.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test client_requests_search_fields_tests
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

- The delete test gives a value that is not `unresolved`.
- A field needs one more query for each row.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/client-requests-task-002-search-row-fields.md`, all of it
- `docs/plans/client-requests-work-plan.md`, items 3 and 6
- The files in "Files To Inspect"

Goal:
- Each search result gives the fields of its row. A test proves the deleted
  album case of ADR 0049 section 3.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.
- Another agent changes `src/api.rs`, `build.rs`, `deploy.sh` and
  `docs/operations.md` in the same working tree. Do not edit those files. If
  a build fails in a file that you did not change, wait one minute and run
  it again.

Do not touch:
- The search rank, filter, cursor and index
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria".
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
