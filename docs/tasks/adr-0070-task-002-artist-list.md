# ADR 0070 Task 002: The Artist List

Owner: [ADR 0070](../adr/0070-a-name-list-gives-each-identity-one-row.md)
sections 3 to 6. v4vmm request 6. Task 001 comes first.

Repository: `stophammer`. The operator commits.

## Goal

`GET /v1/artist-names` gives one `publisher` row for each publisher feed, and
one `name` row for each sort key of the releases of the name level. The rows
come in the order of `sort_name`, then `kind`, then `key`, with `cursor` and
`from`.

## Files To Inspect

- `src/query.rs`: `load_publisher`, `music_to_publisher_facts`,
  `role_agreement`, `rel_tokens`, `link_facts`, `web_url_or_none`, the
  handler of task 001
- `src/db.rs`: `resolve_listed_feed`, `get_feed_remote_items_for_feed`
- `src/openapi.rs`: the path of task 001
- `tests/adr0068_link_facts_tests.rs` and `tests/adr0069_*`: the fixtures

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0070_artist_list_tests.rs`, new
- `docs/API.md`

## Constraints

- **Name level.** A music release is of the name level in two conditions.
  The first condition: no `music_to_publisher` row of its publisher view
  resolves to a feed that the node holds. The second condition: each such row
  has `role_agreement` `both`, and its role tokens do not include `artist`.
- **One rule.** Read the facts with the functions of the publisher view. Do not write a
  second rule.
- **A `name` row.** The key is the sort key of task 001 of the
  `release_artist`. `name` is the raw value that the most releases of the key
  state. With a tie, use the first raw value in code point order. A release
  with a null or empty `release_artist` gives no row.
- **A `publisher` row.** Each feed with the medium `publisher`, also with no
  music feed. `name` is the feed title, and `image_url` passes
  `web_url_or_none`.
- **Order and cursor.** `(sort_name, kind, key)` in code point order. `kind`
  `name` comes before `publisher`. The cursor encodes the three values of the
  last row. `from` works as in task 001.
- **Fields.** `kind`, `name`, `sort_name`, `key`, `feed_guid`, `image_url`,
  `release_count`, as ADR 0070 §3 gives. Register the row type with
  `ToSchema`.
- **Cost.** First calculate the full list at each read, on a copy of the
  backup of 2026-09-26. Report the time of one page. When it is more than
  200 ms, stop and report to the operator before a derived table. A derived
  table needs a migration and a rule for each ingest that changes a link.
- **Contract.** The route is in `spec_value()` with an envelope from
  `envelope_schema`, and `/v1/node/capabilities` lists it.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0070_artist_list_tests.rs`.

- A publisher feed that no music feed names gives a `publisher` row.
- An album that names a held publisher feed, with no `rel`, gives no `name`
  row.
- An album with no publisher link gives a `name` row with `release_count` 1.
- An album whose only link has `role_agreement` `both` and the role `label`
  gives a `name` row.
- An album with a link with the role `artist, label` on both sides gives no
  `name` row.
- Two albums with the artist texts `"Haleen"` and `" haleen "` give one `name`
  row with `release_count` 2.
- A publisher feed with the title `"Haleen"` and a `name` row `"Haleen"` give
  two rows, with the `name` row first.
- Pages of size 2 give each row one time, and `from=h` starts at the first key
  that starts with `h`.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0070_artist_list_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results, and the time of one page
3. behavior changed
4. deviations from this task
5. unresolved concerns
