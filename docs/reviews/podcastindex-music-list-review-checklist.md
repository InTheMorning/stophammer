# Review Checklist: A Daily Pass Reads The PodcastIndex Music List

The orchestrator reviews each diff of
[ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
against this list. The ADR owns each rule. This list only names what to
examine. The plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

## Each Task

| Task | Pass or fail | Required fixes | Merge |
|---|---|---|---|
| 001, the API client | | | |
| 002, the comparison | | | |
| 003, the attempts and the wait | | | |
| 004, the pass and the CLI | | | |
| 005, the measurement | | | |
| 006, the packaging and the documents | | | |

## Rules Of The ADR

- §1: one request to `bymedium?medium=music`, and no request to another
  endpoint. Search the diff for `api.podcastindex.org` and for each path of
  the API, and confirm that only `podcasts/bymedium` appears.
- §2: `PODCASTINDEX_API_KEY` and `PODCASTINDEX_API_SECRET` reach only
  `Credentials::from_env`. No `eprintln!`, `format!`, `tracing` call or
  `Debug` derive takes a credential or a header map. The example env file
  holds `change-me`.
- §3: the pipeline gets `Vec<String>`. No type that reaches
  `run_urls_with_sink`, `FeedSkipDb` or the ingest request holds a field of
  `MusicListRow`.
- §4: the node list comes from `GET /v1/feeds/recent?medium=all`, paged to
  the end, and a failed page stops the pass. The GUID match runs before the
  URL match. `podcast_guid` of a row never becomes the GUID of a feed.
- §5: the wait reads `outcome` and `last_attempted_at` only. `accepted` and
  `no_change` never wait. The mode reads `feed_skip.db` nowhere, and writes
  it through `record_outcome` with the source mode `podcastindex`.
- §6: the three limits are named constants, and each failure prints the
  condition and the numbers, then exits 1 with no URL sent.
- §7: `music_list_attempts` gets a row only from the sink. No code path
  writes a list row before a fetch. The API body is dropped after the parse.
- §8: one report line, printed once, also on a dry run.
- §10: the count reads `import_feed_memory` with `fetch_outcome =
  'accepted'` and `last_attempted_at >= since`. The two tables are written in
  one transaction. No statement of `src/modes/import.rs` changes.

## Conventions

- The crawler lint set is `pedantic = "deny"`. Each `#[expect]` has a
  reason.
- Tests are inline `#[cfg(test)]` modules. No test reaches the real API or
  the real node.
- `run_urls`, `refresh::run` and `batch::run` keep their signatures. The
  existing tests of `batch` and `refresh` pass with no change to their
  assertions.
- `open_state_connection` and `set_import_progress_value` change only in
  visibility.
- Each new module and each `pub(crate)` item has a doc comment that names
  the ADR section.
- Each changed document passes the STE checker with no sentence-length or
  paragraph-length result on the new text.

## Drift To Look For

- A second API request, for example a lookup of a feed by GUID.
- A field of the API row in the ingest request, the skip list or a log line
  that another tool reads.
- A URL match before the GUID match, or a URL match that ignores the
  normalization of §4.
- A wait that reads the skip list, or a skip that has no time limit.
- A change to `import` behavior, other than the two visibilities.
- A credential in a `Debug` output, a `reqwest::Error` text with headers, or
  a test fixture with a real value.
- A document that counts five modes after the merge.

## Merge Recommendation

After the six tasks pass, the orchestrator records:

1. the result of the gate in each crate,
2. the result of the `--dry-run` manual check on the VPS and its numbers,
3. whether the crawler release can be a candidate (ADR 0065), and
4. whether a task packet of §10 needs a change after the first 28 days.
