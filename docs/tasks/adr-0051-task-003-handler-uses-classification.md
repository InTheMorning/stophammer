# ADR 0051 Task 003: The Handler Uses The Classification

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md)
sections 2, 3 and 5. Plan:
[phase plan](../plans/adr-0051-source-url-phase-plan.md), decisions 5 to 8.

## Goal

The ingest handler applies content only in the `Update` and `NewFeed` cases.
The `Mirror` case records an observation only. The two conflict cases write
nothing. The response names the case.

## Files To Inspect

- `docs/adr/0051-feed-content-comes-from-its-source-url.md`, sections 2, 3
  and 5, and "Guards"
- `src/db.rs`: `SubmissionClass`, `classify_submission` (task 002)
- `src/api.rs`: `handle_ingest_feed`, the whole function. Also
  `record_feed_url_observations_for_ingest` and `signed_row_to_event`
- `src/ingest.rs`: `IngestResponse`
- `src/openapi.rs`: the `/ingest/feed` entry and the ingest response shape
- `tests/adr0049_url_observation_tests.rs`
- `tests/adr0051_source_url_tests.rs` (task 001)

## Files Likely To Change

- `src/api.rs`: `handle_ingest_feed` only
- `src/ingest.rs`: `IngestResponse`
- `src/openapi.rs`: the ingest response shape
- `docs/API.md`: the `POST /ingest/feed` section
- `tests/adr0051_source_url_tests.rs`
- Each test that fails because it expects content from a URL that is not the
  source URL

## Do Not Touch

- `src/verify.rs`, `src/verifiers/`, `src/main.rs`
- `classify_submission` itself. If it seems wrong, escalate.
- `src/apply.rs`, `src/event.rs`. No new event type.
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- `IngestResponse` adds `pub source_url: Option<String>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]` and a doc
  comment that names ADR 0051 section 5. Each place that builds an
  `IngestResponse` sets it to `None`, except the mirror case.
- **The write phase.** After the handler takes the writer lock and unwraps
  `feed_data`, and before step 3b, it calls
  `db::classify_submission(&conn, &feed_data.feed_guid, &req.source_url, &req.canonical_url)`:
  - `Update` and `NewFeed`: continue as today.
  - `Mirror { source_url }`: call
    `record_feed_url_observations_for_ingest` as step 11c does. Do not write
    the crawl cache. Return this response:
    - `accepted: false`, `no_change: false`, no warnings,
    - `reason: Some("source_conflict")` and `source_url: Some(source_url)`,
    - the observation event IDs in `events_emitted`,
    - the observation events for fan-out.
  - `RecordConflict`: return `accepted: false`, `no_change: false`,
    `reason: Some("record_conflict")`, no events. Write nothing.
  - `GuidChange { .. }`: the same, with `reason: Some("guid_change_pending")`.
- **The no-change path.** In `ReadPhaseOutcome::NoChange`, after the handler
  takes the writer lock and before it records an observation, it calls
  `classify_submission` with the same arguments. `Update`, `Mirror` and
  `NewFeed` keep today's behavior. `RecordConflict` and `GuidChange` return
  their reason as above, with no observation.
- Put a short comment at each call that names ADR 0051 section 2.
- Log each of the three reasons with `tracing::info!`, with the fields
  `feed_guid`, `canonical_url`, `source_url` and `reason`.
- The OpenAPI document declares `source_url` on the ingest response.
  ADR 0044 requires it. Run `cargo run --bin gen_openapi` and confirm the
  field is in the output.
- `docs/API.md` gives the three reasons and the field.
- **Changed tests.** Run the whole suite. A test that fails because it
  expects content, or `accepted: true`, from a URL that is not the source URL
  of the GUID changes to expect `source_conflict`. Keep each assertion about
  observations and the stored `feed_url`. The report lists each changed test
  with one line on why.

## Implementation Steps

1. Add `source_url` to `IngestResponse`, and set it at each construction.
2. Add the classification to the write phase.
3. Add the classification to the no-change path.
4. Add the tests below to `tests/adr0051_source_url_tests.rs`.
5. Update `src/openapi.rs` and `docs/API.md`.
6. Run the suite. Change only the tests of the rule above.
7. Run the gate.

## Acceptance Criteria

Mechanical. Each item is a test in `tests/adr0051_source_url_tests.rs`, through
`POST /ingest/feed`, with a chain of `["content_hash"]`. Where a test gives
payment routes, use the shape of `IngestPaymentRoute` in `src/ingest.rs`.

- **Mirror.** Feed A at `UA` with GUID `G` and route address `victim@ln.example`.
  Then a body at `UX` with GUID `G`, a different title, a different track and
  route address `attacker@ln.example`.
  - The response is `accepted: false`, `reason: "source_conflict"`,
    `source_url: UA`.
  - The feed title, the track list and each route address are unchanged.
  - `feed_url_observations` maps `UX` to `G`.
  - The assertion message names ADR 0051 section 2.
- **Redirect from the source.** `source_url: UA`, `canonical_url: UY`, GUID
  `G`, a new title. The response is `accepted: true`, and the title changes.
  The stored `feed_url` stays `UA`.
- **GUID change.** `UA` declares a new GUID `GN`. The response is
  `accepted: false`, `reason: "guid_change_pending"`, HTTP `200`. No row in
  `feeds`, `tracks` or `payment_routes` changes.
- **Record conflict.** Feed B at `UB` with GUID `GB`. Then a body at `UB` that
  declares `G`. The response is `record_conflict`. Neither feed changes. The
  `feed_url_observations` rows do not change.
- **No-change conflict.** Feed A accepted at `UA` with hash `H`. Then feed B
  is admitted at `UB`. Then a submission at `UA` with hash `H` that declares
  `GB`. The response is `record_conflict`, not `no_change`, and no observation
  changes.
- **Artist credits.** For each of the three rejections above, the count of
  rows in `artist_credit` is the same before and after.
- The gate is green.
- `cargo run --bin gen_openapi` output contains `source_url` in the ingest
  response.

## Test Commands

```bash
cargo build
cargo test --test adr0051_source_url_tests
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
cargo run --bin gen_openapi | grep -c source_url
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

Add a list of each existing test that changed, with one line on why.

## Escalation Triggers

Stop and report when:

- A test fails for a reason other than content from a URL that is not the
  source URL.
- The write phase does not hold the writer lock from the classification to
  the last write.
- The handler writes a row before the place where the classification goes.
- The name of the artist-credit table is not `artist_credit`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0051-task-003-handler-uses-classification.md`, all of it
- `docs/adr/0051-feed-content-comes-from-its-source-url.md`, sections 2, 3, 5
- `src/db.rs`: `SubmissionClass`, `classify_submission`
- `src/api.rs`: `handle_ingest_feed`, `record_feed_url_observations_for_ingest`,
  `signed_row_to_event`
- `src/ingest.rs`: `IngestResponse`
- `src/openapi.rs`: the `/ingest/feed` entry
- `tests/adr0049_url_observation_tests.rs`, `tests/adr0051_source_url_tests.rs`

Goal:
- `handle_ingest_feed` calls `classify_submission` in the write phase and in
  the no-change path. Only `Update` and `NewFeed` apply content. `Mirror`
  records the observation and answers `source_conflict` with `source_url`.
  The conflict cases answer `record_conflict` or `guid_change_pending` and
  write nothing.

Constraints:
- Follow the "Constraints" section of the task file exactly.
- `IngestResponse.source_url` is optional and skipped when `None`.
- Do not write the crawl cache in a rejected case.
- Change an existing test only when it expects content from a URL that is not
  the source URL. List each change.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/verify.rs`, `src/verifiers/`, `src/main.rs`, `src/apply.rs`, `src/event.rs`
- `classify_submission`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The tests of the "Acceptance Criteria" section of the task file, in
  `tests/adr0051_source_url_tests.rs`.
- The gate is green.
- `cargo run --bin gen_openapi` shows `source_url` on the ingest response.

Test commands:
- `cargo build`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
