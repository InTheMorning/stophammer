# ADR 0074 Task 002: The Crawler Sends No Fallback GUID

Owner: [ADR 0074](../adr/0074-a-feed-without-a-guid-is-invalid.md) §3, §3a
and §3b. The phase plan is
[the compression, splits and GUID plan](../plans/compression-splits-guid-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. ADR 0074 task 001 is
complete. ADR 0072 task 001 also changes `src/crawl.rs`. Do not run the two
tasks at the same time.

## Goal

No mode sends feed data with a GUID that the body does not state. A body that
does not parse is a parse error with its `raw_medium`, and sends no ingest.
The two skip stores keep the rules of the table in ADR 0074 §3b.

## Files To Inspect

- `src/crawl.rs`: `parse_feed_xml`, `build_crawl_report`, `CrawlReport`,
  `crawl_feed_report`, `ingest_cached_feed_report`, `ingest_kept_body`,
  `skip_ingest_with_kept_body`, `post_ingest_payload`, and each function with a
  `fallback_guid` argument
- `src/feed_skip.rs`: `should_skip`, `repeated_failure`, `record_outcome`, and
  the tests
- `src/modes/import.rs`: the call of `crawl_feed_report`, `known_skip_kind`,
  `is_known_medium_gate_rejection`, `is_skip_known_non_music`,
  `ImportMemoryRow`
- `src/modes/ndjson.rs`: `ingest_with_retries` and its caller
- `docs/adr/0030-podcastindex-importer-durable-attempt-memory.md`: the skip
  mode and the 7-day rule

## Files Likely To Change

- `src/crawl.rs`
- `src/feed_skip.rs`
- `src/modes/import.rs`
- `src/modes/ndjson.rs`
- `src/modes/gossip.rs`, `src/modes/batch.rs`: only the call sites that lose the
  `fallback_guid` argument

## Constraints

- **No fallback.** Remove the `fallback_guid` argument from each crawler
  function. `parse_feed_xml` always uses `profile::stophammer()`. The `import`
  mode keeps `row.podcast_guid` for its own memory rows, and does not give it to
  the parse.
- **The parse result.** `parse_feed_xml` gives
  `Result<IngestFeedData, ParseFailure>`, with
  `struct ParseFailure { reason: String, raw_medium: Option<String> }`.
  `reason` is the `Display` text of the parser error. `raw_medium` is
  `ParseError::raw_medium()`. Each parser error is a failure. Remove the path
  that posts `feed_data: null` after a parse.
- **The report.** A `ParseFailure` gives `CrawlOutcome::ParseError(reason)`, and
  `CrawlReport::raw_medium` is the failure `raw_medium`. Add an argument to
  `build_crawl_report` or set the field after it, as the code reads best.
- **The gone report.** `post_gone_report` (ADR 0067) does not change.
- **One rule for the two stores.** Add to `src/feed_skip.rs`:

  ```rust
  pub(crate) fn is_unparsed_channel_without_medium(
      outcome: &str,
      reason: Option<&str>,
      raw_medium: Option<&str>,
  ) -> bool
  ```

  It gives `true` when `outcome` is `parse_error`, `raw_medium` is `None`, and
  `reason` starts with `missing podcast:guid` or `missing <title> element`.
- **The shared skip list.** In `should_skip`, put the new rule after the rule
  "known non-music medium" and before the 7-day rule. A row with
  `fetch_http_status` `200` for which the function gives `true` skips with no
  time limit. The reason is
  `no podcast:medium in a body that does not parse`.
- **The import memory.** `known_skip_kind` gives `KnownSkipKind::Irrelevant`
  also when `fetch_http_status` is `200` and the function gives `true` for the
  memory row. Add the check adjacent to `is_known_medium_gate_rejection`.
- **The other skips do not change.** The skip of a known success, the medium-gate
  rejection, the 7-day rule and the `413` rule keep their code.
- **Comments.** Each change names stophammer ADR 0074 and its section. Delete
  the comment "still POST with `feed_data: null` so the server can record the
  crawl attempt".

## Do Not Touch

- `build_hop_request`, `read_capped_body`, `FetchCounts`
- `src/feed_cache.rs`
- The node, and `../stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Change `parse_feed_xml` and its result type. Follow the compile errors to
   each caller.
2. Remove the `fallback_guid` arguments, and the use in `import` and `ndjson`.
3. Carry `raw_medium` into the report of a parse failure.
4. Add `is_unparsed_channel_without_medium`, and use it in `should_skip` and in
   `known_skip_kind`.
5. Write the tests.

## Acceptance Criteria

Mechanical. Each is a test in the `tests` module of the file that it names.
Each test with an HTTP exchange uses the local stub of `src/crawl.rs`.

- `src/crawl.rs`: a stub body with `podcast:medium` `music` and no
  `podcast:guid` gives `CrawlOutcome::ParseError` and 0 requests to the ingest
  stub. The reason starts with `missing podcast:guid`, and `raw_medium` is
  `music`.
- `src/crawl.rs`: a body with no `<channel>` gives `ParseError` and 0 ingest
  requests.
- `src/feed_skip.rs`, one test for each row of the table of ADR 0074 §3b. Record
  the report with `record_outcome`, then call `should_skip`:

  | Body | `should_skip` |
  |---|---|
  | No GUID, medium `podcast` | `Some`, "known non-music medium: podcast" |
  | No GUID, no medium | `Some`, "no podcast:medium in a body that does not parse" |
  | No title, no medium | `Some`, "no podcast:medium in a body that does not parse" |
  | No GUID, medium `music`, seen now | `Some`, "prior parse error: ..." |
  | No GUID, medium `music`, seen 8 days ago | `None` |

- `src/modes/import.rs`: the same five rows for `known_skip_kind` with
  `skip_known_non_music` set. The first three rows give `Irrelevant`. The last
  two give `None`.
- `src/modes/import.rs`: a memory row with `fetch_outcome` `accepted` still
  gives `KnownSkipKind::Success` with `skip_known_success` set.
- `src/modes/import.rs`: an import row with a PodcastIndex GUID and a stub body
  with no `podcast:guid` gives 0 requests to the ingest stub.
- `src/modes/ndjson.rs`: an export row with a `feed_guid` and a body with no
  `podcast:guid` gives 0 requests to the ingest stub.
- Each existing test passes, or the report names each changed test and the
  reason.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test feed_skip
cargo test known_skip
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A mode needs the null ingest after a parse for a purpose that this task does
  not name.
- An existing test asserts the fallback GUID, or the null ingest, for a
  different reason.
- The `ndjson` mode has no seam for an ingest stub.
- A step needs a change to an ADR, or to a file of "Do Not Touch".

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
- `~/build/stophammer/stophammer-crawler/AGENTS.md`
- `~/build/stophammer/docs/adr/0074-a-feed-without-a-guid-is-invalid.md`, §3, §3a and §3b
- `~/build/stophammer/docs/adr/0030-podcastindex-importer-durable-attempt-memory.md`, the skip mode and the 7-day rule
- `~/build/stophammer/docs/tasks/adr-0074-task-002-crawler-sends-no-fallback.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- No mode sends a fallback GUID. A body that does not parse is a parse error with its `raw_medium`, and sends no ingest. Both skip stores follow the table of ADR 0074 §3b.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- One function, `is_unparsed_channel_without_medium`, gives the new rule to both stores.
- Do not change the gone report or the other skip rules.
- Stop at each escalation trigger of this task.

Do not touch:
- `build_hop_request`, `read_capped_body`, `FetchCounts`, `src/feed_cache.rs`
- the node, `../stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test feed_skip`
- `cargo test known_skip`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
