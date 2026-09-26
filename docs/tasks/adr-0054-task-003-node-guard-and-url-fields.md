# ADR 0054 Task 003: Node Guard And URL Fields

Owner: [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md)
sections 1, 4 and 5. Plan:
[phase plan](../plans/adr-0054-fetch-rule-phase-plan.md), decisions 1, 7, 8
and 9.

Repository: `stophammer`.

## Goal

The node guard rejects each range of ADR 0054. No read route returns a URL
field from RSS whose scheme is not `http` or `https`, and the raw value stays
in the database.

## Files To Inspect

- `src/fetch_guard.rs`: `is_private_ip` and its callers, and its tests
- `tests/sync_register_ssrf_tests.rs`, `tests/ssrf_redirect_tests.rs`
- `src/api.rs`: `handle_ingest_feed`, where the warnings are built
- `src/query.rs`: each response type with a URL field. Find them with
  `grep -n "url" src/query.rs`. They include `image_url`,
  `track_image_url`, `feed_image_url`, `enclosure_url`, the link URLs, the
  remote item URLs and the alternate enclosure URLs.
- `docs/API.md`

## Files Likely To Change

- `src/fetch_guard.rs`, `src/api.rs`, `src/query.rs`, `docs/API.md`
- `tests/adr0054_fetch_rule_tests.rs`, new

## Do Not Touch

- The stored values. No migration, and no change to the ingest write of a
  URL field.
- `feed_url` and `source_url`. They are the address of a record, and the
  ingest classification needs them as they are.
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- **The guard.** Add `pub fn is_public_ip(ip: IpAddr) -> bool` to
  `src/fetch_guard.rs`, with the ranges of task 001 of this ADR, the same
  list. `is_private_ip` becomes `!is_public_ip(ip)`. Its callers do not
  change.
- **The URL helper.** `pub fn web_url_or_none(value: Option<&str>) ->
  Option<String>` in `src/model.rs` or `src/query.rs`. It returns the value
  when it parses as a URL with the scheme `http` or `https`, and `None`
  otherwise.
- **The read routes.** Each response field that carries a URL from RSS goes
  through the helper. One place for each response type, where the type is
  built. The report lists each field and each route.
- **The ingest warning.** Examine each URL field of `IngestFeedData` and of
  each track. For a value that fails the helper, add the warning
  `non-web URL in <field>`, one time for each field name. The submission
  applies as before.
- `docs/API.md` states the rule once, near the start of the read routes, and
  names ADR 0054 section 4.

## Implementation Steps

1. Add `is_public_ip`, and change `is_private_ip`.
2. Add the URL helper, and use it in each response type.
3. Add the ingest warning.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical. Tests in `tests/adr0054_fetch_rule_tests.rs`:

- Each case of ADR 0054 section 5 for `is_public_ip` and for the URL checks
  of `validate_node_url` or the equivalent guard function.
- `is_public_ip` is false for `::ffff:10.0.0.1`, `::127.0.0.1` and
  `64:ff9b::7f00:1`, and true for `::ffff:1.1.1.1`.
- A feed ingested with a feed image `javascript:alert(1)` and a track
  enclosure `data:audio/mp3;base64,AA`: the response has the two warnings,
  the feed is accepted, `GET /v1/feeds/{guid}` returns `null` for both
  fields, and the database holds both raw values.
- A feed with an `https` image returns it unchanged.
- The existing SSRF tests pass.
- The gate is green.

## Test Commands

```bash
cargo build
cargo test --test adr0054_fetch_rule_tests
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

List each response field that now goes through the helper, with its route.

## Escalation Triggers

Stop and report when:

- A URL field reaches a response through a path other than a response type
  in `src/query.rs`, for example a raw JSON column. List it.
- An existing test expects a non-web URL in a response.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0054-task-003-node-guard-and-url-fields.md`, all of it
- `docs/adr/0054-a-fetch-reaches-only-public-feed-hosts.md`
- `docs/plans/adr-0054-fetch-rule-phase-plan.md`, decisions 1, 7, 8 and 9
- The files in "Files To Inspect"

Goal:
- The node guard rejects each range of ADR 0054. No read route returns a
  non-web URL from RSS, and the raw value stays stored.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- The stored values, `feed_url`, `source_url`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of "Acceptance Criteria" in the new file
  `tests/adr0054_fetch_rule_tests.rs`.
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
