# ADR 0081: Measurement Of The First Passes

This record is advisory. It states no binding rule.
[ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
owns the rules. This record gives the first passes in production, and the
reviews of the build.

## The Passes

Release 0.17.0 ran the passes on 2026-10-31. The times are in UTC.

The dry run came first:

```text
podcastindex: import gap: no previous pass, nothing counted
podcastindex: dry run, no URL went to the pipeline
podcastindex: rows=8607 held=8463 candidates=144 waiting=0 sent=0 accepted=0 rejected=0 import_gap=none
```

The first pass with the pipeline started at 18:01:38. Its report line was not
kept. `music_list_attempts` gives its outcomes, because each row of that pass
has its start time:

| Outcome | Feeds |
|---|---|
| `rejected` | 48 |
| `fetch_error` | 46 |
| `parse_error` | 45 |
| `no_change` | 3 |
| `accepted` | 2 |

The second pass with the pipeline started at 20:14:58:

```text
podcastindex: import gap since 1793469698: 0 accepted import feeds not in the music list
fetch: ok=0 not_modified=4 rate_limited=0 other=0 wasted_200=0
podcastindex: rows=8608 held=8466 candidates=142 waiting=139 sent=3 accepted=0 rejected=0 no_change=3 import_gap=0
```

No pass gave a `429` line.

## Comparison With The Context

| Number | Context of the ADR | Production |
|---|---|---|
| List rows | 8,601 | 8,607, then 8,608 |
| Candidates | 142 failed after the crawl of the 31 missing feeds | 144 |
| Rejected by a verifier | 51 | 48 |
| Parse errors, with the 42 feeds with no `podcast:guid` | 45 | 45 |
| Fetch errors | 46 | 46 |

The results:

- **The candidates are the feeds that failed in the crawl of 2026-10-31.**
  The parse errors and the fetch errors agree. The rejections are 3 fewer,
  and 5 candidates gave `accepted` or `no_change`. This record does not
  examine these 5 feeds.
- **The wait of §5 works.** The second pass sent only the 3 `no_change` feeds.
  The 139 failed candidates wait until 2026-11-07.
- **The measurement of §10 works.** The second pass counted 0 accepted
  `import` feeds that the list did not hold, since the start of the first pass.
- **The node list read had no `429`.** The wait of §4 after a `429` did not
  run.

## The Reviews Of The Build

The reviews ran on 2026-10-31, on the working tree before the commit:

| Task | Result | Required fixes |
|---|---|---|
| 001, the API client | Pass | None |
| 002, the comparison | Pass | None |
| 003, the attempts and the wait | Pass | None |
| 004, the pass and the CLI | Pass | Task 005 moved the write of the pass start after the measurement. |
| 005, the measurement | Pass, after one change | The measurement now runs after `check_limits`, so a short list records no count. |
| 006, the packaging and the documents | Pass, with one fix | The PKGBUILD now installs `crawler-podcastindex.env` with mode 600. |
| 007, the wait after a `429` | Pass | None |

A manual check before the release used a mock API with 1,000 rows and a local
node. A pass with the pipeline recorded 1,000 `fetch_error` rows, and the next
dry run gave `waiting=1000`. Two passes in a row made the reader wait 1 s
after a `429`, and both completed.

## Open

- The interval of 3 hours started on 2026-10-31. This record has no pass of
  the timer.
- Task 011 gives the decision of §10 on 2026-11-28.
