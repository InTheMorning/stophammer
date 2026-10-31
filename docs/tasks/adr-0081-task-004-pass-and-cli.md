# ADR 0081 Task 004: The Pass And The `podcastindex` Subcommand

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§1, §3, §5, §6 and §8. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. This task starts
after tasks 001, 002 and 003.

## Goal

`stophammer-crawler podcastindex` does one pass. It reads the music list and
the node list, and classifies the rows. It applies the wait and checks the
limits of §6. It sends the candidates to the `feed` pipeline, records each
outcome, and logs the numbers of §8. `--dry-run` stops before the pipeline.

## Files To Inspect

- `src/modes/batch.rs`: `run_urls` (line 551), `run_waves` (line 434),
  `run_wave` (near line 316) and the `eprintln!` of each outcome (line 357)
- `src/modes/refresh.rs`: `run` (line 237), for the form of a mode entry
  point that reads the node and calls the pipeline
- `src/modes/podcastindex.rs`: the items of tasks 002 and 003
- `src/podcastindex_api.rs`: the items of task 001
- `src/feed_skip.rs`: `FeedSkipDb::open` and `record_outcome`
- `src/main.rs`: `Mode::Refresh`, `Mode::Import`, the `main` dispatch, and
  the test `feed_cache_flag_has_the_expected_env_and_default_on_each_fetching_mode`
- `src/crawl.rs`: `CrawlConfig::from_env_with_force`, for the user agent

## Files Likely To Change

- `src/modes/batch.rs`
- `src/modes/podcastindex.rs`
- `src/modes/import.rs`: `set_import_progress_value` becomes `pub(crate)`
- `src/main.rs`

## Constraints

- **The sink.** In `src/modes/batch.rs`:
  - `pub(crate) type OutcomeSink = Arc<dyn Fn(&str, &CrawlReport) + Send +
    Sync>`.
  - `pub(crate) struct UrlPass { concurrency, host_delay_ms,
    failed_feeds_output, force, feed_cache, revalidate }` with the types of
    the parameters of `run_urls`.
  - `pub(crate) async fn run_urls_with_sink(urls: Vec<String>, pass: UrlPass,
    sink: Option<OutcomeSink>)` holds the body of `run_urls`. `run_urls`
    builds a `UrlPass` and calls it with `None`. No caller of `run_urls`
    changes.
  - `run_waves` and `run_wave` take `sink: Option<&OutcomeSink>`. In
    `run_wave`, after `let report = step(url.clone()).await;`, call the sink
    with the URL that `step` got and the report. The sink gets the outcome of
    each wave, and before the `eprintln!` of the outcome.
- **The pass.** In `src/modes/podcastindex.rs`, `pub async fn run(options:
  PassOptions)` with `pub struct PassOptions { state: String, skip_db:
  String, feed_cache: String, api_base: String, concurrency: usize,
  host_delay_ms: u64, failed_feeds_output: String, force: bool, revalidate:
  bool, dry_run: bool }`. The steps, in this sequence:
  1. `Credentials::from_env()`. An error prints `podcastindex: {err}` and
     exits 1.
  2. Store the start time of the pass in `import_progress` under the key
     `podcastindex_pass_started_at`, after the pass reads the old value into
     a local. Task 005 uses the two values. Make
     `set_import_progress_value` of `src/modes/import.rs` `pub(crate)` for
     this write.
  3. Read the music list with `fetch_music_list`. The client is
     `reqwest::Client::new()`. The user agent is
     `CrawlConfig::from_env_with_force(false).user_agent`.
  4. Read the node list with `collect_feeds` of task 002 over
     `fetch_feeds_recent_page`, from the origin of `INGEST_URL`.
  5. `classify`, then `last_attempts` and `split_waiting` of task 003.
  6. `check_limits`, see below.
  7. When `dry_run` is set, print the report of §8 with the candidate count
     in the place of each pipeline number, and return with exit 0.
  8. Call `run_urls_with_sink` with the URLs of the candidates that do not
     wait, and a sink that does two things for each report: it calls
     `FeedSkipDb::record_outcome(url, report, "podcastindex")`, and, when
     `url` is the URL of a candidate, it calls `AttemptStore::record` with
     the ID of that candidate. The sink reads a `HashMap<String, i64>` from
     candidate URL to ID. It never keys on `report.final_url`.
  9. Print the report of §8.
- **An error stops the pass.** An error from step 3 or step 4 prints
  `podcastindex: {err}` and exits 1, and no URL goes to the pipeline (§4,
  §6).
- **The limits.** `pub(crate) const MIN_LIST_ROWS: usize = 1_000`,
  `MAX_LIST_ROWS: usize = 20_000`, `MAX_DUE_CANDIDATES: usize = 2_000`.
  `pub(crate) fn check_limits(rows: usize, due: usize) -> Result<(), String>`
  gives an error text that names the condition and the numbers, for each
  condition of §6. An error prints `podcastindex: {err}` and exits 1.
- **The report.** `pub(crate) struct PassReport { rows, held, candidates,
  waiting, sent, outcomes: BTreeMap<String, usize> }` with `Display`, printed
  as one line:
  `podcastindex: rows=8601 held=8403 candidates=198 waiting=142 sent=56 accepted=56 rejected=0 ...`.
  The outcome counts come from the sink, for the candidate URLs only.
- **The subcommand.** In `src/main.rs`, `Mode::Podcastindex` with these
  flags and the help text of each:
  - `--state`, default `./import_state.db`
  - `--skip-db`, default `./feed_skip.db`
  - `--feed-cache`, env `FEED_CACHE_DB`, default `./feed_cache.db`
  - `--api-base`, env `PODCASTINDEX_API_BASE`, default `DEFAULT_API_BASE`.
    The help text says that a local test server is its purpose.
  - `--concurrency`, env `CONCURRENCY`, default 5, `parse_positive_usize`
  - `--host-delay-ms`, env `HOST_DELAY_MS`, default 1500
  - `--failed-feeds-output`, env `FAILED_FEEDS_OUTPUT`, default
    `./failed_feeds.txt`
  - `--dry-run`

  The global `--force` and `--no-revalidate` reach `PassOptions` as in the
  other modes. The doc comment of the variant names ADR 0081. Add
  `"podcastindex"` to the mode list of the test
  `feed_cache_flag_has_the_expected_env_and_default_on_each_fetching_mode`.
- **No API field to the node.** The pipeline gets `Vec<String>`. The
  `PassOptions`, the sink and the report hold no `MusicListRow`. A code
  comment at the pipeline call names ADR 0081 §3.
- **The log.** No `eprintln!` in the module takes a credential, a header or
  the `Credentials` struct.

## Do Not Touch

- `src/modes/import.rs`, other than the one visibility change
- `src/modes/gossip.rs`, `src/modes/ndjson.rs`, `src/modes/refresh.rs`
- `src/crawl.rs`, `src/follow.rs`, `src/feed_skip.rs`
- The §10 measurement. Task 005 adds it.
- `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Write `UrlPass`, `OutcomeSink` and `run_urls_with_sink`, and thread the
   sink through `run_waves` and `run_wave`. Run the `batch` tests.
2. Write `check_limits` and `PassReport`, with tests.
3. Write `run`. Put the pass in a function that takes closures for the two
   reads and the pipeline, as `run_corrective_pass` of `refresh` does. Then a
   test can run it with no network.
4. Add the subcommand and the dispatch in `src/main.rs`.
5. Run a `--dry-run` against a local stub API and a local node when one is
   available. Report the result as manual, or report that the step did not
   run.

## Acceptance Criteria

Mechanical. Each is an inline test in the named file.

- `src/modes/batch.rs`: `run_urls_with_sink` gets a sink and a step that
  gives one fixed report. The sink is called one time for each URL of wave 1,
  with that URL. The existing `batch` tests pass with no change to their
  assertions.
- ADR 0081 check 8, `src/modes/podcastindex.rs`: `check_limits(999, 0)`,
  `check_limits(20_001, 0)` and `check_limits(8_601, 2_001)` each give an
  error, and `check_limits(8_601, 2_000)` and `check_limits(1_000, 0)` give
  `Ok`.
- ADR 0081 check 7: the pass function with a node read that fails on the
  second page calls the pipeline closure zero times and gives `Err`.
- The pass function with an API read that fails calls the pipeline zero times
  and gives `Err`.
- ADR 0081 check 6: the pass function gets 3 rows. The node holds 1, and 1
  waits. The pipeline is called one time, with a `Vec<String>` that holds
  only the URL of the due candidate.
- With `dry_run` set, the same inputs call the pipeline zero times and give
  a `PassReport` with `candidates` 2 and `sent` 0.
- `PassReport` `Display` gives one line that starts with `podcastindex: rows=`
  and holds each count.
- `src/main.rs`: `Cli::try_parse_from(["stophammer-crawler", "podcastindex"])`
  parses, with `dry_run` false and `api_base` equal to `DEFAULT_API_BASE`.
  The `feed_cache` test covers `podcastindex`.
- The pass with a sink: after a pipeline closure that calls the sink with one
  `rejected` report for a candidate URL, `AttemptStore` has that ID with the
  outcome `rejected`.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test podcastindex
cargo test batch
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- The sink cannot reach `run_wave` without a change to `run_pool` or to the
  `step` type.
- `clippy::too_many_arguments` fires on `run_urls_with_sink` or on `run`
  after the options struct.
- The pass needs a second API request for a case that the ADR does not name.
- A step needs a change to a file of "Do Not Touch".

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
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §1, §3, §5, §6 and §8
- `~/build/stophammer/docs/tasks/adr-0081-task-004-pass-and-cli.md` (this task)
- `~/build/stophammer/docs/tasks/adr-0081-task-002-comparison.md` and `adr-0081-task-003-attempts-and-wait.md` (the items this task calls)
- the files of "Files To Inspect" in this task

Goal:
- `stophammer-crawler podcastindex` does one pass: the music list, the node list, the classification, the wait, the limits, the pipeline with an outcome sink, and the report line. `--dry-run` stops before the pipeline.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- `run_urls` keeps its signature. The sink comes through `run_urls_with_sink` and `UrlPass`.
- The pipeline gets `Vec<String>` and no API field.
- No log line takes a credential or a header.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/modes/import.rs` other than one visibility, `src/modes/gossip.rs`, `src/modes/ndjson.rs`, `src/modes/refresh.rs`, `src/crawl.rs`, `src/follow.rs`, `src/feed_skip.rs`, `README.md`, `AGENTS.md`
- the §10 measurement
- `stophammer/`, `stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test podcastindex`
- `cargo test batch`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
