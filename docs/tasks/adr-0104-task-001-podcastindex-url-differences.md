# ADR 0104 Task 001: The PodcastIndex Pass Reports A URL On Another Host

Owner: [ADR 0104](../adr/0104-a-moved-feed-is-found-without-a-podping.md) §2,
§3 and §4.

Repository: `stophammer-crawler`, in the folder `stophammer-crawler/` of the
`stophammer` checkout. It has its own Git repository.

## Goal

- The `podcastindex` pass finds each URL difference of ADR 0104 §2.
- The pass replaces the rows of `music_list_url_differences` in
  `import_state.db` with the URL differences of the pass.
- The report line gives `url_differences=N`.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Row classification | extend | `stophammer-crawler/src/modes/podcastindex.rs` | `classify` already finds each row held by its GUID |
| URL comparison | reuse | `stophammer-crawler/src/modes/podcastindex.rs` | `comparable_url` gives the compared form of ADR 0081 §4 |
| State store | extend | `stophammer-crawler/src/modes/podcastindex.rs` | `AttemptStore` owns the tables of `import_state.db` and the transaction pattern of `record_import_gap` |
| Pass report | extend | `stophammer-crawler/src/modes/podcastindex.rs` | `PassReport` gives the report line |

## Files To Inspect

- `stophammer-crawler/src/modes/podcastindex.rs`: all of it, with the tests
- `docs/adr/0104-a-moved-feed-is-found-without-a-podping.md`
- `docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md` §4 to §8
- `stophammer-crawler/AGENTS.md`

## Files Likely To Change

- `stophammer-crawler/src/modes/podcastindex.rs`

## Do Not Touch

- The limits of ADR 0081 §6, the wait of §5 and the import gap of §10.
- `stophammer-crawler/src/modes/refresh.rs` and each mode other than
  `podcastindex`.
- Each file outside `stophammer-crawler/`.

## Constraints

- Add a type `UrlDifference` with these fields: `podcastindex_id: i64`,
  `feed_guid: String`, `node_url: String` and `list_url: String`.
  `feed_guid` is the `feed_guid` of the node feed, not the API value.
- `classify` also returns a `Vec<UrlDifference>`. A row is a URL difference
  when all of these conditions are true:
  - The row is held by its `podcast_guid`, with no regard to letter case.
  - `comparable_url(row.url)` is not equal to `comparable_url` of the
    `feed_url` of the node feed with that GUID.
  - The host of the two compared URLs is not equal. The host is the part of
    the `comparable_url` value before the first `/`, in lowercase.
- A URL difference stays a held row. It is not a candidate. The held count
  does not change.
- Add a table to `AttemptStore::open`:
  `music_list_url_differences (podcastindex_id INTEGER PRIMARY KEY, feed_guid
  TEXT NOT NULL, node_url TEXT NOT NULL, list_url TEXT NOT NULL, pass_at
  INTEGER NOT NULL)`.
- Add `AttemptStore::replace_url_differences(now, &[UrlDifference])`. One
  transaction deletes each row of the table and inserts the new rows. Use the
  pattern of `record_import_gap`.
- `run_pass` calls it after `check_limits` passes and before the pipeline.
  It calls it also in a dry run. A pass that stops at a failed node page, a
  failed API read or `check_limits` does not call it.
- For each URL difference, the pass writes one line to standard error:
  `podcastindex: url difference id=<id> guid=<guid> node=<url> list=<url>`.
- `PassReport` gets `url_differences: usize`. The report line gives
  ` url_differences=N` after `candidates=N`. Update the test of the report
  line and the example line in the doc comments.
- Each new or changed comment names `stophammer` ADR 0104 and its section.
- Read the `rust-dev` skill rules that `stophammer-crawler/AGENTS.md` names.
- Write each comment in Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

Each criterion is a unit test in `podcastindex.rs`:

1. A row held by its GUID, with a URL on another host, gives one URL
   difference with the node `feed_guid`, the node URL and the list URL.
   Input: node `https://feeds.rssblue.com/free`, list
   `https://epochradio.com/feed/tony-salomone-free`.
2. A row held by its GUID at `https://wavlake.com/feed/<uuid>`, with the node
   URL `https://wavlake.com/feed/music/<uuid>`, gives no URL difference.
3. A row held by its GUID, with a URL that differs only in the scheme,
   `www.` or a trailing slash, gives no URL difference.
4. A row held only by its URL gives no URL difference.
5. A URL difference is not a candidate, and the held count includes it.
6. `replace_url_differences` called two times keeps only the rows of the
   second call.
7. A dry run writes the table.
8. A pass that stops at `check_limits` does not change the table. Write a row
   first, run the pass with a list outside the limits, and read the row back.
9. The report line gives `url_differences=N`.

## Checks

Run each command in `stophammer-crawler/`:

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

Report the result of each command. Report each file that you changed.
