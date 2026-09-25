# ADR 0051 Phase Plan: Feed Content Comes From Its Source URL

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md).
This plan states no rule. Phase 1 of the
[remediation plan](feed-trust-remediation-plan.md) points here.

## Goal

A feed publisher cannot change a record that it does not serve, and no
configuration removes the authentication of ingest.

## Non-Goals

The non-goals of ADR 0051 apply. This plan adds none and removes none.

## Assumptions

- Each ingest write holds the one writer lock (`state.db.writer()`), from the
  start of the write phase to the response. PATCH, DELETE and event apply use
  the same lock. Thus a classification after the lock is taken cannot be made
  false by another writer before the ingest writes.
- The crawler reads a `200` response with `accepted: false` as a final
  rejection and does not retry it (`stophammer-crawler/src/crawl.rs:432`).
- The crawler ignores a response field that it does not know.
- The production `VERIFIER_CHAIN` on the VPS can name `crawl_token`. After
  task 001, that value stops the primary at startup.

## Affected Modules

| Crate | File | Task |
|---|---|---|
| `stophammer` | `src/verify.rs`, `src/main.rs`, the ingest handler in `src/api.rs` | 001 |
| `stophammer` | Each test in `tests/` that names `crawl_token` in a `ChainSpec` | 001 |
| `stophammer` | `docs/operations.md`, `docs/verifier-guide.md` | 001 |
| `stophammer` | `src/db.rs`: a new classification function | 002 |
| `stophammer` | The ingest handler in `src/api.rs`, `src/ingest.rs`, `src/openapi.rs`, `docs/API.md` | 003 |
| `stophammer` | `tests/adr0049_url_observation_tests.rs` and each other test that expects content from a second URL | 003 |
| `stophammer-crawler` | `src/crawl.rs`: the node answer after an ingest | 004 |

## Decisions For The Tasks

These decisions settle each open point before a task starts. A task that
finds a point not in this list stops and reports it.

1. **The authentication stays in `VerifierChain`, outside the list.**
   `build_chain(spec, crawl_token)` keeps its signature. It puts the token
   check in a separate field of `VerifierChain`, not in the configurable
   list. `VerifierChain::new` takes the token as its first argument.

   A new method, `VerifierChain::authenticate(&IngestFeedRequest)`, returns
   the same failure text as today: `[crawl_token] invalid crawl token`. The
   handler calls it first, before the reader connection. The 47 `AppState`
   values in the tests keep their fields.
2. **`crawl_token` in the list is a startup panic.** `build_chain` panics with
   a message that names ADR 0051 section 4 and tells the operator to remove
   the name from `VERIFIER_CHAIN`. `ChainSpec::DEFAULT` drops the name.
3. **An empty token is a startup failure.** A new function,
   `verify::require_crawl_token(value: &str) -> Result<(), String>`, rejects an
   empty value or a value of white space only. `main.rs` calls it and returns
   a `StartupError`. `build_chain` also panics on an empty token, so no code
   path makes a chain without one.
4. **The classification is one function in `src/db.rs`.**
   `classify_submission(conn, feed_guid, source_url, canonical_url)` returns
   a `SubmissionClass` enum with the five cases of ADR 0051 section 2, in
   that order. It only reads.
5. **The handler classifies in two places.** The write phase classifies after
   it takes the writer lock and before step 3b. The no-change path
   classifies before it records an observation. In the no-change path,
   `Update`, `Mirror` and `NewFeed` keep today's behavior, and the two
   conflict cases return their reason with no observation.
6. **The mirror result.** The node records the URL observations with the
   current `record_feed_url_observations_for_ingest`, and fans out their
   events. It does not write the hash cache. It answers `accepted: false`,
   `no_change: false`, `reason: "source_conflict"` and
   `source_url: Some(<stored feed_url>)`, with the observation event IDs in
   `events_emitted`.
7. **The conflict results.** `record_conflict` and `guid_change_pending`
   answer `accepted: false` with the reason, no event and no write.
8. **The response field.** `IngestResponse` adds
   `source_url: Option<String>` with
   `#[serde(default, skip_serializing_if = "Option::is_none")]`.
9. **The crawler leaves the node answer empty for a conflict.** When the
   rejection reason is exactly `source_conflict`, `record_conflict` or
   `guid_change_pending`, the crawler does not call `record_node_answer`. It
   clears an answer that the row holds from an earlier ingest.
   ADR 0050 already treats an empty answer as "submit the kept body after a
   `304`".

## Sequence

| Task | Crate | Needs |
|---|---|---|
| [001](../tasks/adr-0051-task-001-authentication-first.md) Authentication first | `stophammer` | Nothing |
| [002](../tasks/adr-0051-task-002-classify-submission.md) The classification function | `stophammer` | 001 |
| [003](../tasks/adr-0051-task-003-handler-uses-classification.md) The handler uses the classification | `stophammer` | 002 |
| [004](../tasks/adr-0051-task-004-crawler-keeps-conflicts-open.md) The crawler keeps a conflict open | `stophammer-crawler` | Nothing |
| [005](../tasks/adr-0051-task-005-deploy-and-repair.md) Deploy and repair | VPS | 001 to 004 |
| [006](../tasks/adr-0051-task-006-replay-fetch-cache.md) Repair from the fetch cache | `stophammer-crawler`, VPS | 005 step 6 |

Tasks 001 and 004 can run at the same time. They change different
repositories. Tasks 001, 002 and 003 change the same files in sequence.

## Schema And API Implications

- No migration. No new table and no new column.
- `IngestResponse` gets one optional field. The OpenAPI document declares it
  (ADR 0044).
- Three new rejection reasons. The envelope does not change.
- `VERIFIER_CHAIN` loses a permitted name. An operator value that names
  `crawl_token` stops the primary.
- No new event type. The mirror case emits `FeedUrlObserved`, as today.

## Risk Areas

- **The production `VERIFIER_CHAIN`.** If the VPS names `crawl_token`, the
  primary does not start after the deploy. Task 005 examines the value first.
- **Wavlake URL forms.** One album has two URL forms. After task 003, content
  from the second form is a mirror. The first form must still be crawled, or
  the album gets no update. The `refresh` mode reads the stored URLs, so it
  crawls the first form.
- **Tests that expect content from a second URL.** Task 003 changes them. A
  test that fails for a different reason is an escalation, not an edit.
- **A NoChange path that records an observation for a conflicting URL.**
  Decision 5 closes it.

## Test Strategy

- Unit tests in `src/verify.rs` for decisions 1 to 3.
- Unit tests in `src/db.rs` or `tests/db_tests.rs` for each case of decision 4.
- A new integration file, `tests/adr0051_source_url_tests.rs`, for the Guards
  of ADR 0051 through `POST /ingest/feed`.
- Inline tests in `stophammer-crawler/src/crawl.rs` for decision 9.
- The full gate in each crate after each task.

## Rollback

- Tasks 001 to 003 are one node binary. To roll back, deploy the previous
  image. The previous binary accepts the same requests and has the takeover
  defect again.
- A rollback does not undo the repair of task 005. The repaired rows are
  correct content from the source URLs.
- Task 004 alone is safe to roll back. The crawler then stores conflicts as
  `rejected`, and a conflict waits for a changed body.
