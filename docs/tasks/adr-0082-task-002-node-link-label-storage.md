# ADR 0082 Task 002: The Node Stores The Label Of A Link

Owner: [ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md) §2 and §3,
with [ADR 0023](../adr/archive/0023-schema-migrations.md) for
the migration. The phase plan is
[the funding links plan](../plans/funding-links-phase-plan.md).

Repository: `stophammer`. The operator commits. This task is independent of
task 001, because the node has its own ingest types.

## Goal

The node stores the label of a link, and gives a `funding` link the source
`podcast_funding`. It emits a new `SourceEntityLinksReplaced` event when only
a label changed. A migration adds the column.

## Context

`IngestLink` in `src/ingest.rs` is the type that the crawler posts.
`SourceEntityLink` in `src/model.rs` is the stored type, and the payload of
`SourceEntityLinksReplaced` in `src/event.rs` holds a list of it. An event
keeps its `payload_json` as signed, so a new field changes no stored event.
Several payload types already use
`#[serde(default, skip_serializing_if = "Option::is_none")]` for an optional
field.

`src/api.rs` maps each `IngestLink` to a `SourceEntityLink` with the source
`rss_link`. `src/db.rs` reads the links in three functions, writes them in
`replace_source_entity_links_for_feed`, removes a duplicate key in
`dedupe_source_entity_links`, and compares two sets in
`source_entity_links_changed`. The dedupe keeps the first row of a key.

Migration `0057_feed_host.sql` shows the form of a column addition.

## Files To Inspect

- `src/ingest.rs`: `IngestLink`
- `src/model.rs`: `SourceEntityLink`
- `src/event.rs`: `SourceEntityLinksReplacedPayload`, and one field with
  `skip_serializing_if`
- `src/api.rs`: the function that builds `SourceEntityLink` rows with the
  source `rss_link`
- `src/db.rs`: `get_source_entity_links_for_feed`,
  `get_source_entity_links_for_feed_entity`,
  `get_source_entity_links_for_entity`, `replace_source_entity_links_for_feed`,
  `dedupe_source_entity_links`, `source_entity_links_changed`
- `src/schema.sql`: `source_entity_links`
- `migrations/0057_feed_host.sql`, `tests/fixtures/migration_sha256.txt`
- `tests/adr0054_host_name_link_tests.rs`, for an ingest with links
- `tests/apply_tests.rs`, for an event assertion

## Files Likely To Change

- `src/ingest.rs`, `src/model.rs`, `src/db.rs`, `src/api.rs`, `src/schema.sql`
- New: `migrations/0058_source_link_label.sql`
- `tests/fixtures/migration_sha256.txt`
- New: `tests/adr0082_funding_link_storage_tests.rs`
- Each file with a `SourceEntityLink { ... }` literal: `src/model.rs`,
  `src/db.rs`, `src/api.rs`, `tests/apply_tests.rs`, `tests/db_tests.rs`

## Constraints

- **The ingest type.** `IngestLink` gets `#[serde(default)] pub label: Option<String>`.
- **The stored type.** `SourceEntityLink` gets `pub label: Option<String>`
  with `#[serde(default, skip_serializing_if = "Option::is_none")]`. The
  reason, in a doc comment: a stored payload has no field, and a payload with
  no label keeps its bytes.
- **The migration.** `migrations/0058_source_link_label.sql` has a comment
  that names ADR 0082 §2, then
  `ALTER TABLE source_entity_links ADD COLUMN label TEXT;`. `src/schema.sql`
  adds `label TEXT` as the last column of the table, because `ALTER` appends.
  `tests/adr0023_migration_hash_tests.rs` gives the line to add to the
  fixture.
- **The reads and the write.** Each `SELECT` that builds a `SourceEntityLink`
  reads `label`. The `INSERT` writes it.
- **The compare.** `source_entity_links_changed` compares `label`. The dedupe
  changes not.
- **The source.** The `api.rs` mapping gives `podcast_funding` when
  `link_type` is `funding`, and `rss_link` otherwise. It copies the label.
- **Lint.** The crate lint set is in `Cargo.toml`. No broad `#[allow]`.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `src/query.rs`, `src/openapi.rs`, `tests/fixtures/v1-schema-fields.json`
- `docs/`
- `src/event.rs`, other than a read. The payload type holds the list, and the
  list type carries the field.
- `stophammer-parser/`, `stophammer-crawler/`

## Implementation Steps

1. Add the field to `IngestLink` and `SourceEntityLink`. Set `label: None` in
   each literal.
2. Write the migration, change `src/schema.sql`, run
   `cargo test --test adr0023_migration_hash_tests`, and add the line it gives.
3. Change the three reads, the write and the compare in `src/db.rs`.
4. Change the mapping in `src/api.rs`.
5. Write `tests/adr0082_funding_link_storage_tests.rs`.
6. Run the gate.

## Acceptance Criteria

Mechanical, in `tests/adr0082_funding_link_storage_tests.rs`:

- A JSON `SourceEntityLink` with no `label` key deserializes with a null
  label. A link with a null label serializes with no `label` key. A link with
  a label serializes with it.
- An ingest with a `website` link and a `funding` link with a label stores two
  rows. The `funding` row has the source `podcast_funding` and the label. The
  `website` row has the source `rss_link` and a null label.
- A second ingest of the same feed, with only the label of the funding link
  changed, adds a `SourceEntityLinksReplaced` event.
- An ingest with two `funding` links of the same URL and different labels
  stores one row with the first label.

Mechanical, in the crate:

- `cargo test --test adr0023_migration_hash_tests` and
  `cargo test --test migration_tests` pass.
- `cargo test` passes, with no change to an existing assertion other than a
  `label: None` in a literal.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`
  pass.

## Test Commands

```bash
cd ~/build/stophammer
cargo build
cargo test --test adr0023_migration_hash_tests
cargo test --test migration_tests
cargo test --test adr0082_funding_link_storage_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- A test reports a skipped migration, because an existing database recorded
  the version. AGENTS.md "New Database Migration" step 4 gives the repair, and
  the orchestrator decides it.
- A signature test fails after the field is added. The field must change no
  stored payload.
- An existing test asserts the exact column list of `source_entity_links`.

## Expected Final Report

1. files changed and created
2. tests run, with the result of the gate
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0082-a-funding-link-is-a-source-link.md`, §2 and §3
- `~/build/stophammer/docs/tasks/adr-0082-task-002-node-link-label-storage.md` (this task)
- `~/build/stophammer/AGENTS.md`, "New Database Migration" and "Code Style"
- the files of the section "Files To Inspect" of this task

Goal:
- `SourceEntityLink`, `IngestLink` and the table `source_entity_links` have an optional `label`. A `funding` link gets the source `podcast_funding`. A label change gives a new `SourceEntityLinksReplaced` event.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write each comment and doc comment in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- Use the lint set of `Cargo.toml`. No broad `#[allow]`.

Do not touch:
- `src/query.rs`, `src/openapi.rs`, `tests/fixtures/v1-schema-fields.json`
- `docs/`
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
