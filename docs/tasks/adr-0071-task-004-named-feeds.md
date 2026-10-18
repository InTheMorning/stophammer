# ADR 0071 Task 004: The Named Feeds Of A Release Row

Owner: [ADR 0071](../adr/0071-a-list-row-links-what-it-names.md) section 3.
v4vmm request 8, gap B.

Repository: `stophammer`. The operator commits.

## Goal

With `include=named_feeds`, each music row of `GET /v1/feeds/recent` and
`GET /v1/feeds/by-name` gives `named_feeds`.

## Files To Inspect

- `src/query.rs`: `FeedResponse`, `feed_list_row_to_response`,
  `load_publisher`, `PublisherResponse`, `album_item_rank`,
  `FEED_LIST_INCLUDES`, `handle_capabilities`
- `src/openapi.rs`: the paths `/v1/feeds/recent` and `/v1/feeds/by-name`
- `tests/adr0068_link_facts_tests.rs`, `tests/adr0069_rank_tests.rs`: the
  fixtures

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0071_named_feeds_tests.rs`, new
- `docs/API.md`

## Constraints

- **Entries.** One entry for each `music_to_publisher` row of
  `load_publisher` for the feed, in the sequence of `album_names_rank`. Each
  entry gives `publisher_feed_guid`, `remote_feed_title`, `album_names_rank`,
  `role` and `role_agreement`, with the values of that row.
- **A feed that the node does not hold** gives an entry with a null
  `remote_feed_title`.
- **Empty.** A music feed that names no publisher feed gives an empty list.
  A row of another medium gives no `named_feeds`.
- **Schema.** The entry is a new type with `ToSchema`, registered as ADR 0044
  gives. Rule 1a holds for it.
- **Rule 1c.** Add `named_feeds` to `FEED_LIST_INCLUDES`.
- A request without the include gives the rows of today.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The role rules and the rank rule
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add the entry type and the `named_feeds` field.
2. Fill it in `feed_list_row_to_response` from `load_publisher` when the
   include is present.
3. Write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0071_named_feeds_tests.rs`.

- An album that names A and then B gives two entries, A with rank 1 and B with
  rank 2. The `role` and `role_agreement` of each entry equal the values of
  `GET /v1/feeds/{guid}?include=publisher`.
- An album whose first item names a feed that the node does not hold gives an
  entry with a null `remote_feed_title`.
- A music feed with no publisher item gives an empty list. A publisher row
  gives no `named_feeds`.
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

- A page of 200 rows with the include takes more than 10 times the time of
  a page without it. Measure on the newest backup in `backups/`.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. the page time with and without the include
5. deviations from this task
6. unresolved concerns
