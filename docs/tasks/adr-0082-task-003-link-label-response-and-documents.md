# ADR 0082 Task 003: The Response Gives The Label

Owner: [ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md) §4, with
the archived [ADR 0044](../adr/archive/0044-api-contract-declares-its-fields.md)
for the contract guards and the archived
[ADR 0054](../adr/archive/0054-a-fetch-reaches-only-public-feed-hosts.md) §4
for the URL rule. The phase plan is
[the funding links plan](../plans/funding-links-phase-plan.md).

Repository: `stophammer`. The operator commits. This task starts after task
002, because it reads `SourceEntityLink.label`.

## Goal

Each `source_links` row of the `v1` feed route and track route gives `label`.
A `funding` row follows ADR 0054 §4 only. The contract fixture holds the field,
and `docs/API.md` and `docs/schema-reference.md` describe it.

## Context

`SourceEntityLinkResponse` in `src/query.rs` is the response type, and
`entity_link_response` builds it. The function gives `website` and `web_page`
the rule of ADR 0054 §4a through `web_link_for_read`. Each other type takes
`web_url_or_none`, which gives null for a scheme other than `http` or `https`.
A `funding` row must take that second arm.

The OpenAPI document derives the schema from the struct. The fixture
`tests/fixtures/v1-schema-fields.json` lists the properties of each `v1`
schema, and `tests/adr0044_field_removal_tests.rs` fails when a listed
property is gone.

`docs/API.md` gives a `source_links` example in the feed response and in the
track response, and a paragraph on the URL rules before "Common query
parameters". `docs/schema-reference.md` has a short section on
`source_entity_links`.

## Files To Inspect

- `src/query.rs`: `SourceEntityLinkResponse`, `entity_link_response`,
  `web_link_for_read`, `web_url_or_none`
- `tests/fixtures/v1-schema-fields.json`: `SourceEntityLinkResponse`
- `tests/adr0044_field_removal_tests.rs`, `tests/adr0044_schema_refs_tests.rs`
- `docs/API.md`: the `source_links` examples, and the paragraph that names
  ADR 0054 section 4a
- `docs/schema-reference.md`: `source_entity_links`
- `tests/adr0054_host_name_link_tests.rs`, for a read of
  `include=source_links`

## Files Likely To Change

- `src/query.rs`
- `tests/fixtures/v1-schema-fields.json`
- `docs/API.md`, `docs/schema-reference.md`
- New: `tests/adr0082_funding_link_response_tests.rs`

## Constraints

- **The field.** `SourceEntityLinkResponse` gets `label: Option<String>` after
  `url_scheme_added`, with a doc comment that names ADR 0082 §4: the node value
  of a `podcast:funding` tag, null for each other `link_type`.
  `entity_link_response` copies `link.label`.
- **The scheme rule.** The `match` in `entity_link_response` changes not. A
  comment at the `_` arm names ADR 0082 §4: a `funding` row follows section 4
  only.
- **The fixture.** `SourceEntityLinkResponse.properties` gets `label`, in the
  alphabetical place of the list.
- **The examples of `docs/API.md`.** Each `source_links` example row gets
  `"label": null`. The feed example gets one more row: a `funding` link with
  a label, the source `podcast_funding` and the path `feed.podcast:funding`.
- **The prose of `docs/API.md`.** The paragraph on section 4a gets two
  sentences: a `funding` row follows section 4 only, and ADR 0082 §4 owns this
  rule. A new short paragraph gives `label`, the type `funding`, the source
  `podcast_funding` and the two extraction paths. It names ADR 0082 §1 and
  §2.
- **`docs/schema-reference.md`.** The section `source_entity_links` gets a
  note on `label` and on the `funding` row, with the ADR.
- **The OpenAPI document.** `cargo run --bin gen_openapi` runs with no error,
  and the output holds `label` in `SourceEntityLinkResponse`.
- **Prose.** Each changed document passes the STE checker with no
  sentence-length or paragraph-length result on the new text.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `src/db.rs`, `src/api.rs`, `src/model.rs`, `src/ingest.rs`, `migrations/`
- `src/openapi.rs`, other than a read. The schema derives from the struct.
- `api.html`. It names no `source_links` field.
- `stophammer-parser/`, `stophammer-crawler/`

## Implementation Steps

1. Add the field and the comment in `src/query.rs`.
2. Add `label` to the fixture.
3. Write `tests/adr0082_funding_link_response_tests.rs`.
4. Change `docs/API.md` and `docs/schema-reference.md`.
5. Run the gate, `gen_openapi` and the STE checker.

## Acceptance Criteria

Mechanical, in `tests/adr0082_funding_link_response_tests.rs`. One ingest
posts a feed with four feed links and one track link:

- a `website` link,
- a `funding` link `https://example.com/donate` with a label,
- a `funding` link with a `javascript:` URL and a label,
- a `funding` link `www.example.com/donate` with no scheme,
- a track `funding` link with a label.

Then:

- `GET /v1/feeds/{guid}?include=source_links` gives the five rows. The
  `website` row has a null `label`. The first `funding` row has its `label`
  and its URL. The `javascript:` row has a null `url`. The no-scheme row has a
  null `url` and `url_scheme_added` false.
- The track route with `include=source_links` gives the track `funding` row
  with its `label`.

Mechanical, in the crate:

- `cargo test` passes, with the ADR 0044 guards.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`
  pass.
- `cargo run --bin gen_openapi | grep -c '"label"'` gives 1 or more.

Visual, for the operator:

- The `docs/API.md` examples read as one response, and the paragraph on the
  URL rules names both ADRs.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0082_funding_link_response_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c '"label"'
python3 "$HOME/.agents/skills/asd-ste100/scripts/ste_lint.py" --check --no-heuristics docs/API.md docs/schema-reference.md
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- `tests/adr0044_schema_refs_tests.rs` needs a change of `src/openapi.rs`
  for the new field.
- The track route gives no `source_links` for a track link in the test. That
  is a defect outside this task.

## Expected Final Report

1. files changed and created
2. tests run, with the result of the gate and of the STE checker
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0082-a-funding-link-is-a-source-link.md`, §4
- `~/build/stophammer/docs/tasks/adr-0082-task-003-link-label-response-and-documents.md` (this task)
- `~/build/stophammer/AGENTS.md`, "New API Endpoint" steps 3 to 5
- the files of the section "Files To Inspect" of this task

Goal:
- Each `source_links` row gives `label`. A `funding` row follows ADR 0054 §4 only. The fixture and the two documents hold the field.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write each comment and each document change in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Use the lint set of `Cargo.toml`. No broad `#[allow]`.

Do not touch:
- `src/db.rs`, `src/api.rs`, `src/model.rs`, `src/ingest.rs`, `migrations/`, `api.html`
- `stophammer-parser/`, `stophammer-crawler/`
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. files changed and created
2. tests run
3. behavior changed
4. deviations from this task
5. unresolved concerns
