# ADR 0068 Task 001: The Link Facts Of A Publisher Row

Owner: [ADR 0068](../adr/0068-a-publisher-row-gives-its-link-facts.md).
musicindex.org request 7. Release 0.4.0. ADR 0059 task 002 comes first,
because both tasks change `src/query.rs`.

Repository: `stophammer`. The operator commits.

## Goal

With `include=link_facts`, each publisher row of `GET /v1/feeds/recent`
gives `two_way_link_count`, `stated_rels` and `confirmed_release_artists`.
The search part was removed from ADR 0068 on 2026-10-01, before the build.
ADR 0038 keeps publisher feeds out of the search index.

## Files To Inspect

- `src/query.rs`: `ListQuery` and `includes`, the handler of
  `GET /v1/feeds/recent` (it builds `FeedResponse` rows), `load_publisher`,
  `PublisherResponse`
  (`direction`, `two_way_validated`, `publisher_rel`),
  `confirmed_and_unconfirmed_release_artists`, `FEED_INCLUDES` and
  `handle_capabilities`
- `tests/adr0061_confirmed_artists_tests.rs`: the helpers that store a
  publisher feed with linked albums
- `docs/API.md`: the list route and the capabilities route

## Files Likely To Change

- `src/query.rs`, `docs/API.md`
- `tests/adr0068_link_facts_tests.rs`, new

## Do Not Touch

- `load_publisher` and the publisher row builders.
- The full feed read, and the default output of each route without
  `include=link_facts`.
- The storage, the ingest.

## Constraints

- One helper computes the three values for one feed from the rows of
  `load_publisher`, with the rules of ADR 0068 §1:
  - `two_way_link_count`: the count of rows with `direction`
    `publisher_to_music` and `two_way_validated` true.
  - `stated_rels`: the different non-null `publisher_rel` values of those
    rows, raw, sorted, with no split or change.
  - `confirmed_release_artists`: the result of
    `confirmed_and_unconfirmed_release_artists` for the same rows.
- Call the helper only when `include=link_facts` is given, and only for a row
  whose `raw_medium` is publisher by `medium::is_publisher`. Each field uses
  `#[serde(skip_serializing_if = "Option::is_none")]`.
- Add a constant for the include names of the list route.
  Make `handle_capabilities` list it, as it lists `FEED_INCLUDES`. Name the
  key in `docs/API.md`.
- Each new field and the helper have doc comments that name ADR 0068.

## Measurement

Before the report, measure on a copy of the production data:

1. Copy `backups/stophammer-20260926T131633Z.db` to `target/`.
2. Open the copy with the node built in release mode. Bind it to `127.0.0.1`
   on a free port. Use throwaway token values and a key path in `target/`.
3. Time `GET /v1/feeds/recent?medium=publisher&limit=200&include=link_facts`
   and the same without `include`, 5 times each. Report the median of each.
4. Stop the node. Delete the copy.

When the median with `include` is more than 1 second, report it as an
escalation. ADR 0068 §4 gives that decision to the operator.

### Result On 2026-10-01

The copy of 2026-09-26 gave 200 publisher rows. The largest row had 52
two-way links. Each form ran 5 times on a warm node:

| Request | Median |
|---|---|
| Without `include` | 0.011 s |
| With `include=link_facts` | 0.095 s |

The median with `include` is less than 1 second, so no escalation is
necessary. The first request of each form ran on a cold cache and took up to
1.6 seconds. The node gave no panic.

## Acceptance

Mechanical, each an integration test in `tests/adr0068_link_facts_tests.rs`:

- A publisher feed with two two-way links, one with `rel="label"`, and one
  link that is not two-way: a list row with
  `include=link_facts` gives `two_way_link_count` 2 and `stated_rels`
  `["label"]`. The full feed read gives the same counts.
- The list row gives the same `confirmed_release_artists` as the full read.
- A music feed row with `include=link_facts` gives none of the three fields.
- A list row without the include gives none of the three fields.
- `GET /v1/node/capabilities` lists `link_facts`.
- The guards of ADR 0044 pass. The gate is green.

## Test Commands

```bash
cd /home/citizen/build/stophammer
cargo build
cargo test --test adr0068_link_facts_tests
cargo test --test adr0044_schema_refs_tests
cargo test --test adr0044_contract_guard_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report without a workaround when:

- The median of the measurement with `include` is more than 1 second.
- A guard of ADR 0044 needs a change to the guard.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- /home/citizen/build/stophammer/docs/tasks/adr-0068-task-001-link-facts.md
- /home/citizen/build/stophammer/docs/adr/0068-a-publisher-row-gives-its-link-facts.md
- /home/citizen/build/stophammer/AGENTS.md (Code Style, Tests)
- Only the parts of src/query.rs named in the task file. Use grep. The files are long.

Goal:
- With include=link_facts, each publisher row of /v1/feeds/recent gives two_way_link_count, stated_rels and confirmed_release_artists. Measure the cost on a copy of the production data.

Constraints:
- The rules under "Constraints" and "Measurement" in the task file.
- Write any scratch file under target/, not /tmp. For a lint exception use #[expect], not #[allow].

Do not touch:
- load_publisher and the publisher row builders, the full feed read, the default output of each route, the storage, the ingest, stophammer-crawler/, stophammer-parser/.
- Git: run no git command that writes (no add, commit, stash, checkout, reset, push).

Acceptance criteria:
- The criteria under "Acceptance" in the task file.

Test commands:
- The commands under "Test Commands" in the task file.

At the end, report:
1. files changed
2. the real output of each gate command, and the measurement
3. behavior changed
4. deviations from task
5. unresolved concerns
