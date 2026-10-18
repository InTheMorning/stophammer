# ADR 0071 Task 001: The Row Link Guards And The Feed GUID Of A Release Row

Owner: [ADR 0071](../adr/0071-a-list-row-links-what-it-names.md) sections 1a,
1c and 3 (`publisher_feed_guid` only). v4vmm request 8.

Repository: `stophammer`. The operator commits.

## Goal

Two guards over the OpenAPI document enforce rules 1a and 1c. Each row of
`GET /v1/feeds/recent` and `GET /v1/feeds/by-name` gives `publisher_feed_guid`.
The document declares `include` on `GET /v1/feeds/by-name`.

## Files To Inspect

- `src/query.rs`: `FeedResponse`, `resolve_publisher_feed_title`,
  `feed_list_row_to_response`, `handle_get_feeds_by_name`, `FEED_INCLUDES`,
  `FEED_LIST_INCLUDES`, `TRACK_INCLUDES`, `handle_capabilities`
- `src/openapi.rs`: `spec_value`, the paths `/v1/feeds/recent` and
  `/v1/feeds/by-name`
- `tests/adr0044_contract_guard_tests.rs`: the pattern of a guard over the
  document

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0071_row_link_guard_tests.rs`, new
- `docs/API.md`

## Constraints

- **Rule 1a.** For each schema of the document, each property
  `<prefix>feed_title` needs the property `<prefix>feed_guid` in the same
  schema. The guard holds a list of each pair with other names. Today the list
  has one pair: `CopyRemoteItemResponse` gives `remote_feed_title` with
  `resolved_feed_guid`.
- **Rule 1c.** For each route that reads `include`, the names that the code acts
  on are the names that the document declares for that route. The constants
  `FEED_INCLUDES`, `FEED_LIST_INCLUDES` and `TRACK_INCLUDES` are the source of
  the code side.
- **Unknown names.** A route continues to ignore an unknown `include` name with
  `200`. Do not add a `400`.
- **`publisher_feed_guid`.** It is the GUID of the feed that gives
  `publisher_feed_title`, from the same resolution. It is null when the title
  is null. Change `resolve_publisher_feed_title` to give both values, with no
  second database read.
- **Messages.** Each guard failure names "ADR 0071 §1a" or "ADR 0071 §1c" and
  the fix, as the guards of ADR 0044 do.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The behavior of each `include` name, and the role rules
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write the two guards. Run them, and record that rule 1a fails on
   `FeedResponse.publisher_feed_title` and rule 1c fails on
   `/v1/feeds/by-name`.
2. Add `publisher_feed_guid` to `FeedResponse`.
3. Declare `include` with the names of `FEED_LIST_INCLUDES` on
   `/v1/feeds/by-name`.
4. Run the guards again. Then update `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0071_row_link_guard_tests.rs`.

- The rule 1a guard passes. A test schema with a `<prefix>feed_title` and no
  GUID property makes the guard function fail, with a message that names ADR
  0071 §1a.
- The rule 1c guard passes. The document declares `link_facts` on
  `/v1/feeds/by-name`.
- A release row whose album names a held publisher feed gives
  `publisher_feed_guid` equal to the GUID of that feed. A row with a null
  `publisher_feed_title` gives a null `publisher_feed_guid`.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0071_row_link_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- The rule 1a guard finds a second pair with other names, or a title with no
  GUID that this task does not name.
- A route acts on an `include` name that no constant lists.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns
