# ADR 0081 Task 002: The Comparison Matches By GUID First

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§4. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. This task has no
dependency on task 001. Run it before task 003 when one agent does both,
because the two tasks start `src/modes/podcastindex.rs`.

## Goal

The mode reads the feeds that the node holds, with their GUIDs. It
classifies each row of the music list as held or as a candidate. A row is
held by its GUID first, then by its URL after the normalization of §4.

## Files To Inspect

- `src/modes/refresh.rs`: `FeedsRecentItem`, `FeedsRecentPage`,
  `fetch_feeds_recent_page`, `collect_feed_urls`, `run_corrective_pass` and
  their tests
- `src/podcastindex_api.rs`: `MusicListRow`, when task 001 is complete. When
  it is not, define the same struct in the new module and name the move in
  the report.
- `src/url_queue.rs`: `host_key`, for the form of a URL helper
- `docs/API.md` of `stophammer`, section `GET /v1/feeds/recent`: `feed_guid`
  and `feed_url` on each row

## Files Likely To Change

- `src/modes/refresh.rs`
- `src/modes/podcastindex.rs`, new
- `src/modes/mod.rs`: one `pub mod podcastindex;` line

## Constraints

- **The node list reader.** In `src/modes/refresh.rs`, add `feed_guid:
  String` to `FeedsRecentItem`. Make `FeedsRecentItem`, `FeedsRecentPage`,
  `FeedsRecentPagination`, `fetch_feeds_recent_page`,
  `query_origin_from_ingest_url` and `ingest_url_from_env` `pub(crate)`. Add
  `pub(crate) async fn collect_feeds<F, Fut>(fetch_page: &mut F) ->
  Result<Vec<FeedsRecentItem>, String>` with the loop of `collect_feed_urls`.
  Make `collect_feed_urls` call it and map each item to its URL. The
  behavior of `refresh` does not change.
- **The node feed.** In `src/modes/podcastindex.rs`, `pub(crate) struct
  NodeFeed { feed_guid: String, feed_url: String }`, with
  `From<FeedsRecentItem>`.
- **The normalization.** `pub(crate) fn comparable_url(url: &str) -> String`
  does these steps in this sequence:
  1. Trim the text.
  2. Remove `http://` or `https://` at the start.
  3. Remove `www.` at the start of what remains.
  4. Remove each `/` at the end.
  5. Lowercase the text before the first `/`. The text after the first `/`
     keeps its case.

  A URL with no scheme goes through the same steps.
- **The classification.** `pub(crate) struct Classification { held: usize,
  candidates: Vec<MusicListRow> }` and `pub(crate) fn classify(rows:
  Vec<MusicListRow>, node: &[NodeFeed]) -> Classification`. Build one set of
  the lowercase `feed_guid` values and one set of the `comparable_url` values
  of the node. A row is held when its `podcast_guid` in lowercase is in the
  first set, or when `comparable_url(&row.url)` is in the second set. Each
  other row is a candidate, in the sequence of the list. A row with no
  `podcast_guid` is compared by URL only.
- **No node write and no fetch.** This task adds no HTTP call to the node
  other than the existing list read, and no feed fetch.
- **Comments.** The module doc of `src/modes/podcastindex.rs` names ADR 0081
  and says that task 004 adds the pass. Each `pub(crate)` item has a doc
  comment that names its section.

## Do Not Touch

- `src/modes/batch.rs`, `src/modes/import.rs`, `src/modes/gossip.rs`
- `src/main.rs`, other than nothing. The subcommand comes in task 004.
- `src/podcastindex_api.rs`
- `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Change `src/modes/refresh.rs`: the field, the visibilities and
   `collect_feeds`. Run the `refresh` tests.
2. Write `comparable_url` and its tests.
3. Write `NodeFeed`, `Classification` and `classify`, and their tests.
4. Add the module line.

## Acceptance Criteria

Mechanical. Each is an inline test in `src/modes/podcastindex.rs`, unless the
item names a different file.

- `src/modes/refresh.rs`: `collect_feeds` over two pages gives each item with
  its `feed_guid` and its `feed_url`. The existing tests of
  `run_corrective_pass` and `collect_feed_urls` pass with no change to their
  assertions.
- `comparable_url("https://www.Wavlake.com/feed/music/ABC/")` gives
  `wavlake.com/feed/music/ABC`.
- `comparable_url("http://thebearsnare.com/zza.xml")` and
  `comparable_url("https://thebearsnare.com/zza.xml")` give the same text.
- `comparable_url("wavlake.com/feed/x")` gives `wavlake.com/feed/x`.
- ADR 0081 check 3: a row with the `podcastGuid` `ABCDEF01-...` is held when
  the node holds `abcdef01-...` at a different URL.
- ADR 0081 check 4: a row at `https://wavlake.com/feed/<uuid>` with the GUID
  of a node record at `https://wavlake.com/feed/music/<uuid>` is held.
- ADR 0081 check 5: a row with a `podcastGuid` that the node does not hold,
  at `https://www.example.com/feed/`, is held when the node holds
  `http://example.com/feed`.
- A row with no `podcastGuid` is held by URL, and is a candidate when no URL
  matches.
- A row that matches neither set is a candidate, and `candidates` keeps the
  sequence of the list.
- `held + candidates.len()` is equal to the number of rows.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test podcastindex
cargo test refresh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- A test of `refresh` must change its assertion because of the new field.
- `GET /v1/feeds/recent` has a row with no `feed_guid` in a test body, and
  the deserialization must decide a default.
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
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §4
- `~/build/stophammer/docs/tasks/adr-0081-task-002-comparison.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- The mode reads the node feeds with their GUIDs and classifies each row of the music list as held or as a candidate. GUID first, then the normalized URL.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- `refresh` keeps its behavior. Only visibilities, one field and one shared function change there.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/modes/batch.rs`, `src/modes/import.rs`, `src/modes/gossip.rs`, `src/main.rs`, `src/podcastindex_api.rs`, `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test podcastindex`
- `cargo test refresh`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
