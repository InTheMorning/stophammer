# ADR 0076 Task 001: The Confirmation Of A Named Feed Entry

Owner: [ADR 0076](../adr/0076-a-named-feed-entry-states-its-confirmation.md)
section 1. v4vmm request 9, gap A.

Repository: `stophammer`. The operator commits.

## Goal

Each `named_feeds` entry of `GET /v1/feeds/recent` and `GET /v1/feeds/by-name`
gives `two_way_validated`.

## Files To Inspect

- `src/query.rs`: `NamedFeedEntry`, `NamedFeedEntry::from_rows`,
  `PublisherResponse`, `is_confirmed_link`
- `tests/adr0071_named_feeds_tests.rs`: the fixtures
- `tests/fixtures/v1-schema-fields.json`

## Files Likely To Change

- `src/query.rs`
- `tests/adr0071_named_feeds_tests.rs`
- `tests/fixtures/v1-schema-fields.json`
- `docs/API.md`

## Constraints

- **Value.** The value is the `two_way_validated` of the same
  `music_to_publisher` row of `load_publisher`. Do not calculate it again.
- **No other field.** Add no state field. The other five fields do not change.
- **Schema.** Add the property to `tests/fixtures/v1-schema-fields.json`, as
  ADR 0044 gives.
- A request without `include=named_feeds` gives the rows of today.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The confirmation rule of ADR 0049, and the rank rule
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the field to `NamedFeedEntry`, and copy it in `from_rows`.
2. Write the tests, then update the schema fixture and `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0071_named_feeds_tests.rs`.

- An album that names a feed that also lists the album gives an entry with
  `two_way_validated` true.
- An album that names a held feed that does not list the album gives an entry
  with `two_way_validated` false.
- An album that names a feed that the node does not hold gives an entry with
  `two_way_validated` false and a null `remote_feed_title`.
- The value of each entry equals the value of
  `GET /v1/feeds/{guid}?include=publisher` for the same link.
- The ADR 0044 guards and the ADR 0071 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0071_named_feeds_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- The `publisher` view row and the entry give different values for one link.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
