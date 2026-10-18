# ADR 0054 Task 005: A Link With No Scheme Reads As `https`

Owner: [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md)
section 4a. musicindex.org request 15.

Repository: `stophammer`. The operator commits. Task 006 of ADR 0064 comes
after this task and uses its function.

## Goal

Some stored links of the type `website` or `web_page` have no scheme and
start with a host name. A read gives such a value as `https://` and the
value. The row gives `url_scheme_added`.

## Files To Inspect

- `src/model.rs`: `web_url_or_none`
- `src/query.rs`: `entity_link_response`, `SourceEntityLinkResponse`
- `src/openapi.rs`: the schema of `SourceEntityLinkResponse`
- `tests/adr0054_fetch_rule_tests.rs`: the URL field tests of task 003

## Files Likely To Change

- `src/model.rs`, `src/query.rs`, `src/openapi.rs` (only when the schema is
  not generated from the struct)
- `tests/adr0054_host_name_link_tests.rs`, new
- `docs/API.md`

## Constraints

- **One function.** Add `pub fn web_link_for_read(raw: &str) -> (Option<String>, bool)`
  in `src/model.rs`, next to `web_url_or_none`. It gives the read value and
  true when it added the scheme. It follows the five steps of ADR 0054 §4a
  exactly. Task 006 uses the same function.
- **Host name.** The part before the first `/`, `?` or `#` has two or more
  labels with a `.` between them. Each label has 1 to 63 ASCII letters,
  digits or `-`, and does not start or end with `-`. The last label has two
  or more ASCII letters and nothing else. Do not use a list of top-level
  domains.
- **Scope.** Only a link row with `link_type` `website` or `web_page` uses
  the new function. Each other link row and each other URL field keeps
  `web_url_or_none`, and gives `url_scheme_added` false.
- **Field.** `SourceEntityLinkResponse` gets `url_scheme_added: bool`. Each
  link row gives it, on the feed read and on the track read.
- **Storage.** The stored raw value does not change. No migration.

## Do Not Touch

- `stophammer-crawler/`, `stophammer-parser/`
- `docs/adr/`, `docs/plans/`, `AGENTS.md`
- `web_url_or_none` and each caller of it, except `entity_link_response`
- Each migration file
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write `web_link_for_read` with unit tests for each case of the acceptance
   criteria.
2. In `entity_link_response`, use it for `website` and `web_page`, and set
   `url_scheme_added`.
3. Add the field to the OpenAPI schema when the schema needs it.
4. Write the integration test, then `docs/API.md`.

## Acceptance Criteria

Mechanical. The unit tests are in `src/model.rs`. The integration tests are in
`tests/adr0054_host_name_link_tests.rs`.

- `www.pixelpalace.party` gives `(Some("https://www.pixelpalace.party"), true)`.
- `strangeloveofmusic.wixsite.com/strangelove` gives the `https://` form and
  true.
- `https://example.com/` gives `(Some("https://example.com/"), false)`.
- `The Fraglebay`, `localhost`, `example`, `example.c0m`, `-a.com` and
  `javascript:alert(1)` give `(None, false)`.
- `  www.TheSynthesatsers.com ` gives `https://www.TheSynthesatsers.com` and
  true.
- A feed read with `include=source_links` of a feed with
  `<link>www.pixelpalace.party</link>` gives the `website` row with
  `url` `https://www.pixelpalace.party` and `url_scheme_added` true.
- A `self_feed` row with no scheme gives `url` null and `url_scheme_added`
  false.
- The ADR 0044 guards pass.

## Test Commands

```bash
cargo build
cargo test --lib web_link_for_read
cargo test --test adr0054_host_name_link_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A step needs a change to an ADR, or to a file of "Do Not Touch".
- A constraint of this task disagrees with the code.
- An existing test fails after the change, and the cause is not clear.

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `AGENTS.md` (the code style and the pre-commit gate)
- `docs/adr/0054-a-fetch-reaches-only-public-feed-hosts.md`, sections 4 and 4a
- `docs/tasks/adr-0054-task-005-host-name-link.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A `website` or `web_page` link row gives a stored value with no scheme that starts with a host name as `https://` and the value, and each link row gives `url_scheme_added`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- One function, `model::web_link_for_read`, makes the read value.
- The stored raw value does not change. No migration.
- Stop at each escalation trigger of this task.

Do not touch:
- `stophammer-crawler/`, `stophammer-parser/`, `docs/adr/`, `docs/plans/`, `docs/tasks/`, `AGENTS.md`
- `web_url_or_none` and each caller of it, except `entity_link_response`
- each migration file
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test --lib web_link_for_read`
- `cargo test --test adr0054_host_name_link_tests`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
