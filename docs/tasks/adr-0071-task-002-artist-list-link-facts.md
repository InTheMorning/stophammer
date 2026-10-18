# ADR 0071 Task 002: The Link Facts Of An Artist List Row

Owner: [ADR 0071](../adr/0071-a-list-row-links-what-it-names.md) section 2.
v4vmm request 8, gap A.

Repository: `stophammer`. The operator commits.

## Goal

With `include=link_facts`, each `publisher` row of `GET /v1/artist-names` gives
`two_way_link_count`, `stated_rels`, `agreed_roles`,
`confirmed_release_artists` and `first_named_count`.

## Files To Inspect

- `src/query.rs`: `ArtistNameRow`, `build_artist_name_rows`,
  `ArtistNameCache`, `ArtistNameCacheEntry`, `handle_get_artist_names`,
  `link_facts`, `LinkFacts`, `is_confirmed_link`, `load_publisher`,
  `album_item_rank`, `FEED_LIST_INCLUDES`, `handle_capabilities`
- `src/openapi.rs`: the path `/v1/artist-names`
- `tests/adr0070_artist_list_tests.rs`, `tests/adr0068_link_facts_tests.rs`:
  the fixtures

## Files Likely To Change

- `src/query.rs`, `src/openapi.rs`
- `tests/adr0071_artist_list_link_facts_tests.rs`, new
- `docs/API.md`

## Constraints

- **Same values.** The four ADR 0068 fields come from `link_facts` over the rows
  of `load_publisher`, as on `GET /v1/feeds/recent`.
- **`first_named_count`.** Add it to `LinkFacts`. It counts the confirmed
  `publisher_to_music` rows that `two_way_link_count` counts, with
  `album_names_rank` 1. `GET /v1/feeds/recent` and `GET /v1/feeds/by-name` then
  give it on a publisher row with `include=link_facts` too.
- **In memory.** The build of ADR 0070 §4a calculates the facts. A page reads
  them from the copy in memory. A request without the include gives the rows
  of today, with no new field.
- **A `name` row** gives none of the five fields.
- **Rule 1c.** Declare `include` with `link_facts` on `/v1/artist-names`, and
  list it in `GET /v1/node/capabilities`.
- No migration, no event change and no ingest change.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- The role rules, the rank rule, and the name level rule of ADR 0070 §4
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Measure the time of one build of the artist list on the newest backup in
   `backups/`, with `?immutable=1` on a copy. Record it.
2. Add `first_named_count` to `LinkFacts`.
3. Calculate the facts of each publisher feed in the build, and keep them in
   the copy in memory.
4. Give them on the row with the include. Measure the build time again.
5. Write the tests, then `docs/API.md`.

## Acceptance Criteria

Mechanical. Each is a test in `tests/adr0071_artist_list_link_facts_tests.rs`.

- With the include, a `publisher` row gives the same four ADR 0068 values as
  `GET /v1/feeds/recent?medium=publisher&include=link_facts` for that feed.
- An album that names the feed first, with a confirmed link, adds 1 to
  `first_named_count`. An album that names it second does not. An album that
  names it first with a link that is not confirmed does not.
- A `name` row gives none of the fields. A request without the include gives
  none of the fields.
- The ADR 0044 guards and the ADR 0071 guards pass.

## Test Commands

```bash
cargo build
cargo test --test adr0071_artist_list_link_facts_tests
cargo test --test adr0070_artist_list_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report, with no workaround, when:

- The build time on the backup becomes more than two times the time of step 1.
- The facts need a rule that ADR 0068 or ADR 0069 §3a does not state.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. the two build times on the backup
5. deviations from this task
6. unresolved concerns
