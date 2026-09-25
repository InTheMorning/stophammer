# ADR 0051 Task 001: Authentication First

Owner: [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md)
section 4. Plan: [phase plan](../plans/adr-0051-source-url-phase-plan.md),
decisions 1, 2 and 3.

## Goal

The node checks the crawl token before any database read. No value of
`VERIFIER_CHAIN` and no value of `CRAWL_TOKEN` removes that check.

## Files To Inspect

- `src/verify.rs`: `VerifierChain`, `VerifierChain::new`, `VerifierChain::run`,
  `ChainSpec::DEFAULT`, `ChainSpec::from_env`, `build_chain`, the inline tests
- `src/verifiers/crawl_token.rs`
- `src/main.rs`: lines 80 to 95, the `CRAWL_TOKEN` read and `build_chain`
- `src/api.rs`: `handle_ingest_feed`, from its start to the line
  `let read_outcome = {`
- `tests/adr0049_url_observation_tests.rs`: `test_app_state`, as one example
  of a test chain

## Files Likely To Change

- `src/verify.rs`
- `src/main.rs`
- `src/api.rs`: `handle_ingest_feed` only
- Each file in `tests/` that names `"crawl_token"` in a `ChainSpec`, or calls
  `VerifierChain::new`. Find them with:
  `grep -rln '"crawl_token"\|VerifierChain::new' tests`
- `docs/operations.md`, `docs/verifier-guide.md`: the `VERIFIER_CHAIN` values
- `tests/adr0051_source_url_tests.rs`, new

## Do Not Touch

- `src/verifiers/crawl_token.rs`. The comparison stays as it is.
- `src/db.rs`, `src/ingest.rs`, `src/openapi.rs`
- The ingest handler after the line `let read_outcome = {`
- `stophammer-crawler`, `stophammer-parser`

## Constraints

- `VerifierChain` gets a separate token check that is not in the
  `verifiers` list. `VerifierChain::new(crawl_token: String, verifiers)`
  takes the token first. It panics when the token is empty or white space
  only.
- A new method, `VerifierChain::authenticate(&self, req: &IngestFeedRequest)
  -> Result<(), VerifierError>`, uses `verifiers::crawl_token::token_matches`
  for the comparison. Review replaced `CrawlTokenVerifier` with that
  function, because no chain can hold the verifier after this task. The error text is exactly `[crawl_token] invalid crawl token`,
  the same as today.
- `build_chain(spec, crawl_token)` keeps its signature. It passes the token
  to `VerifierChain::new`.
- `build_chain` panics when `spec.names` contains `crawl_token`. The panic
  text names `ADR 0051 section 4` and says: remove `crawl_token` from
  `VERIFIER_CHAIN`, because the node always checks the token first.
- `ChainSpec::DEFAULT` becomes
  `content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type`.
- The panic text for an unknown verifier no longer lists `crawl_token` as
  valid.
- A new public function, `verify::require_crawl_token(value: &str) ->
  Result<(), String>`, returns an error for an empty or white-space value.
  The error names ADR 0051 section 4.
- `main.rs` calls `require_crawl_token` after it reads `CRAWL_TOKEN`, and
  maps the error to `startup_error`.
- `handle_ingest_feed` calls `state2.chain.authenticate(&req)` inside the
  blocking closure, before `state2.db.reader()`. A failure returns the same
  `IngestResponse` as a verifier rejection today: `accepted: false`,
  `no_change: false`, `reason: Some(<error text>)`, no events, no warnings.
- The tests keep their behavior. Remove each `"crawl_token"` entry from a
  `ChainSpec` list. Change each `VerifierChain::new(vec![])` to
  `VerifierChain::new("<token>".into(), vec![])`. Use the token that the test
  already sends in its payloads. A test that sends no ingest can use any
  non-empty token.
- Put a short comment at the new check that names ADR 0051 section 4.

## Implementation Steps

1. Change `VerifierChain`, `new`, `build_chain`, `ChainSpec::DEFAULT` and the
   panic text in `src/verify.rs`. Add `authenticate` and
   `require_crawl_token`.
2. Update the inline tests in `src/verify.rs`, and add the unit tests below.
3. Call `require_crawl_token` in `src/main.rs`.
4. Call `authenticate` first in `handle_ingest_feed`.
5. Update the test files that the grep finds.
6. Update the `VERIFIER_CHAIN` values and text in `docs/operations.md` and
   `docs/verifier-guide.md`. State that the node always checks the crawl
   token first and that `crawl_token` is not a valid chain name. Name ADR
   0051 section 4.
7. Create `tests/adr0051_source_url_tests.rs` with the integration test below.
8. Run the gate.

## Acceptance Criteria

Mechanical:

- The gate is green.
- A unit test: `build_chain` with `crawl_token` in the names panics, and the
  panic text contains `ADR 0051`.
- A unit test: `require_crawl_token("")` and `require_crawl_token("   ")`
  return `Err`. `require_crawl_token("abc")` returns `Ok`.
- A unit test: `VerifierChain::new(String::new(), vec![])` panics.
- A unit test: `ChainSpec::DEFAULT` does not contain `crawl_token`.
- An integration test in `tests/adr0051_source_url_tests.rs`: a chain of
  `["content_hash"]` only, a feed accepted once, then the same URL and the
  same `content_hash` with a wrong token. The response has
  `accepted: false` and the reason `[crawl_token] invalid crawl token`, not
  `no_change`. The `feed_url_observations` row count does not change.
- The count of `#[test]` and `#[tokio::test]` functions in `tests/` does not
  go down, except for tests that only tested `crawl_token` as a chain name.
  The report names each removed test.

## Test Commands

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- A test fails for a reason other than the chain construction or the token.
- A caller of `VerifierChain::new` or `build_chain` exists in `src/` other
  than `build_chain` and `main.rs`.
- The change needs an edit to the handler after `let read_outcome = {`.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0051-task-001-authentication-first.md`, the sections
  "Constraints" and "Acceptance Criteria"
- `src/verify.rs`
- `src/verifiers/crawl_token.rs`
- `src/main.rs`, lines 80 to 95
- `src/api.rs`: `handle_ingest_feed`, from its start to `let read_outcome = {`
- `tests/adr0049_url_observation_tests.rs`: `test_app_state`, `ingest_payload`,
  `ingest_response`

Goal:
- The node checks the crawl token first, outside the configurable verifier
  list. `crawl_token` in `VERIFIER_CHAIN` and an empty `CRAWL_TOKEN` stop
  startup.

Constraints:
- `VerifierChain::new(crawl_token, verifiers)` holds the token apart from the
  list, and panics on an empty or white-space token.
- `VerifierChain::authenticate(&IngestFeedRequest)` uses `token_matches`
  and returns the error text `[crawl_token] invalid crawl token`.
- `build_chain` keeps its signature. It panics when a name is `crawl_token`,
  with a message that names ADR 0051 section 4.
- `ChainSpec::DEFAULT` drops `crawl_token`.
- `verify::require_crawl_token(&str) -> Result<(), String>` rejects an empty
  or white-space value. `main.rs` calls it.
- `handle_ingest_feed` calls `authenticate` before `state2.db.reader()`, and a
  failure returns `accepted: false` with that reason, no events.
- Update each test chain: remove `"crawl_token"` from `ChainSpec` names, and
  pass the token that the test sends to `VerifierChain::new`.
- Update `docs/operations.md` and `docs/verifier-guide.md`.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `src/verifiers/crawl_token.rs`, `src/db.rs`, `src/ingest.rs`, `src/openapi.rs`
- The ingest handler after `let read_outcome = {`
- `stophammer-crawler`, `stophammer-parser`

Acceptance criteria:
- The gate is green.
- Unit tests:
  - A chain name `crawl_token` panics with `ADR 0051` in the text.
  - `require_crawl_token` rejects `""` and `"   "`, and accepts `"abc"`.
  - `VerifierChain::new(String::new(), vec![])` panics.
  - `ChainSpec::DEFAULT` has no `crawl_token`.
- Integration test in the new file `tests/adr0051_source_url_tests.rs`: with a
  chain of `["content_hash"]`, a repeat submission with the same hash and a
  wrong token is rejected with `[crawl_token] invalid crawl token`, and the
  observation count does not change.
- No test is lost except tests of `crawl_token` as a chain name. Name each
  one that you remove.

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
