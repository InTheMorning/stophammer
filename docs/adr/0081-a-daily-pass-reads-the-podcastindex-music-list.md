# ADR 0081: A Daily Pass Reads The PodcastIndex Music List

## Status
Proposed on 2026-10-31. The operator answers the questions in "Open
Questions" before the status changes.
[The phase plan](../plans/podcastindex-music-list-phase-plan.md) and tasks 001
to 006 in `docs/tasks/` give the build. They take the proposed values of the
open questions until the operator answers.

## Date
2026-10-31

## Context

The crawler finds a new feed in three ways, and each way misses a group of
music feeds:

- `gossip` fetches a feed after a podping. Wavlake sends no podping. When the
  shared skip list holds a medium rejection for a URL, the mode skips that URL
  with no time limit. `--skip-ttl-days` sets a limit (`src/feed_skip.rs`).
- `import` reads the PodcastIndex snapshot and fetches each PodcastIndex ID one
  time. Its cursor does not go back (ADR 0030, ADR 0033). The snapshot has no
  `medium` column, so the mode must fetch a row to learn its medium.
- `refresh` reads only the feeds that the node holds (ADR 0047).

The PodcastIndex API gives the music list in one request:
`GET /api/1.0/podcasts/bymedium?medium=music`. Probe runs on 2026-10-31 gave
these facts:

| Fact | Value |
|---|---|
| Feeds in one response | 8,601, with no paging. The API ignores `max`. |
| Size of the response | 9.8 MB, in approximately 3 s |
| Feeds with `dead` set in the list | 0 |
| `feedsWithMediumMusic` in `stats.podcastindex.org/daily_counts.json` | 10,017. The difference can be dead feeds. The probe did not confirm this. |
| `medium=publisher`, `medium=musicL` | "No feeds in the index for this medium." |
| Feeds in one `POST /podcasts/batch/byguid` response | At most 25, for each number of GUIDs |
| `GET /recent/feeds` in 24 hours | 1,000 rows, the maximum. 0 rows were in the music list. |
| `GET /recent/newfeeds` | 1,000 rows in 24 hours. 19 of 20 sample rows had the medium `podcast`. |
| Authentication | Four headers: `User-Agent`, `X-Auth-Key`, `X-Auth-Date` and `Authorization`, which is the SHA-1 of the key, the secret and the date. The date must be within 3 minutes. |
| Rate-limit headers | None |

The list was compared with the backup of 2026-10-25, by GUID and by URL. It
had 198 music feeds that the node did not hold. A crawl of the 198 URLs into
a copy of that backup, with the default verifier chain, gave these results:

| Result | Feeds |
|---|---|
| Accepted | 56 |
| Rejected by `v4v_payment` | 51 |
| No `podcast:guid` (ADR 0074) | 42 |
| Other parse errors | 3 |
| Fetch errors: `404`, `410`, `403` and network | 46 |

The 56 accepted feeds give three causes of a missed feed:

1. **The snapshot is old.** On 2026-10-31, the snapshot had the date
   2026-10-24.
   - 25 feeds came into production after the backup. Their PodcastIndex IDs
     are 8,084,872 or lower.
   - 24 Wavlake albums, released from 2026-10-26 to 2026-10-30, were not in
     production on 2026-10-31. Their IDs are 8,087,452 or higher.

   Inference: a new Wavlake album comes into the index only through a new
   snapshot and an `import` run.
2. **A feed or a rule changed after the one fetch.** Four feeds had a medium
   rejection in `import_feed_memory` and the skip list in March and April 2026.
   The feeds state `music` now. One feed was rejected by `v4v_payment` in April
   2026, and ADR 0048 now accepts it. No mode fetched these five feeds again.
3. **The node database changed.** One feed was accepted on 2026-04-18. The
   event log of the present database starts on 2026-04-19, and the feed was
   not in it.

One more feed has no record in either state file, and its cause is not known.

The operator crawled the 31 missing feeds with the `feed` mode on 2026-10-31.
The node accepted each one. The follow waves accepted 13 artist feeds and 205
albums.

Two more facts constrain a comparison:

- For 927 Wavlake music records, the node holds the self link
  `wavlake.com/feed/music/<uuid>` (ADR 0052), and PodcastIndex holds
  `wavlake.com/feed/<uuid>` or `www.wavlake.com/feed/<uuid>`. A match by URL
  misses these records. A match by GUID finds them.
- PodcastIndex assigns a GUID to each feed, also to a feed with no
  `podcast:guid` (`musicindex-research`,
  `research_notes/aggregation/podcastindex-guid-behavior.md`). The node
  refuses such a feed (ADR 0074).

The terms of service of PodcastIndex constrain the use of the API.
§5.5(1) does not let a user "build databases" from API content. §4.2(1) says
that "Developer credentials may not be embedded in open source projects."

## Decision

### 1. A crawler mode reads the music list

`stophammer-crawler` gets a mode `podcastindex`. One run does one pass and
stops. The operator runs the pass one time each day.

A pass sends one API request: `GET /api/1.0/podcasts/bymedium?medium=music`.
It sends no request to another PodcastIndex endpoint.

### 2. The credentials stay outside the repository

The mode reads the key from `PODCASTINDEX_API_KEY` and the secret from
`PODCASTINDEX_API_SECRET`. It never writes either value to a log, a file or an
error message. No file in the three repositories holds a real value.

### 3. The API gives URLs, never content

The mode uses three fields of each list row: `id`, `url` and `podcastGuid`. It
sends each URL that the node does not hold to the pipeline of the `feed` mode
(`modes::batch`). The pipeline fetches the feed, parses it and sends it to
`POST /ingest/feed`, with the fetch cache (ADR 0050), the host delay and the
follow waves.

No field of the API response goes to the node. The fetched RSS body is the only
source of the data that the node stores (ADR 0006, ADR 0030).

### 4. The comparison reads the node and matches by GUID first

The mode reads the feeds that the node holds from
`GET /v1/feeds/recent?medium=all`, as `refresh` does (ADR 0047). A page that
fails stops the pass, and the mode sends no URL to the pipeline.

A list row is held when one of these conditions is true:

- Its `podcastGuid` is equal to the `feed_guid` of a node feed, with no regard
  to letter case.
- Its `url` is equal to the `feed_url` of a node feed. Before the mode
  compares the two values, it removes the scheme, a leading `www.` and a
  trailing slash from each one.

Each other row is a candidate.

The mode does not use `podcastGuid` as the GUID of a feed. The fetched body
gives the GUID, and a body with no `podcast:guid` is a parse error (ADR 0074).

### 5. A candidate waits 7 days after a failed try

The mode keeps one row for each candidate that it sent to the pipeline, in a
table `music_list_attempts` of `import_state.db`. The row holds the
PodcastIndex ID, the URL, the outcome, the reason and the time of the last try.

A candidate with an outcome other than `accepted` or `no_change` waits 7 days.
Then the next pass tries it again. No outcome stops the tries for all time,
because a feed and a verifier rule can change after a try (Context, cause 2).

The mode does not read the shared skip list `feed_skip.db`. It writes the
outcome of each fetch to it, as the other modes do.

### 6. A list that is not normal stops the pass

The pass stops, and sends no URL to the pipeline, in each of these conditions:

- The API answer is not `200`, or its JSON `status` is not `true`.
- The list has fewer than 1,000 rows.
- The list has more than 20,000 rows.
- More than 2,000 rows are candidates that do not wait under §5.

The log gives the condition and the numbers. The operator examines the cause
before the next pass.

### 7. The mode stores only its own results

The mode stores no list row. `music_list_attempts` holds only the rows of
candidates that it fetched, with the result of its own fetch. The mode keeps no
copy of the API response after the pass.

### 8. The pass reports its numbers

At the end of a pass, the log gives these numbers:

- list rows, held rows and candidates,
- candidates that wait, and candidates sent to the pipeline,
- each outcome of the pipeline.

### 9. Out of scope

- **Dead feeds.** The music list holds no dead feed, so the API cannot tell
  that a held feed is dead. ADR 0067 stays the only way to retire a gone
  source.
- **Publisher feeds and `musicL` feeds.** The API gives no list for these
  mediums. The follow waves stay the way to find them.
- **Changes to held feeds.** The comparison sees only feeds that the node does
  not hold. `refresh` (ADR 0047) gets the changes of held feeds that send no
  podping.
- **The other endpoints.** `recent/feeds`, `recent/newfeeds`, `recent/data`,
  `bytag` and `batch/byguid` are not used. The Context gives the reasons.

### 10. A measurement decides the schedule of `import`

`import` fetches each snapshot row, whatever its medium. Thus it can find a
feed whose RSS states `music` when PodcastIndex gives another medium. The
music list does not hold such a feed. The number of such feeds is not known.

Each pass measures this gap:

1. The pass reads the rows of `import_feed_memory` with the outcome `accepted`
   and a `last_attempted_at` after the start of the previous pass.
2. It counts each row whose `podcastindex_id` is not in the list of this pass.
3. The log gives the count. Two tables of `import_state.db` keep the result:
   `import_gap_passes` keeps the time and the count of each pass, and
   `import_gap_rows` keeps the ID and the URL of each counted row.

These rows are results of `import` fetches, so §7 permits them.

After 28 days of passes, the operator examines the two tables:

- When the counted rows are none or almost none, a new ADR stops the schedule
  of `import` and amends ADR 0030 and ADR 0033. `import` stays available as a
  manual tool. It needs no API key, and it can fetch the full corpus again
  after a loss of node data.
- When the counted rows are real music feeds, `import` keeps its schedule. The
  counted rows give the size of the gap.

`AGENTS.md` names this measurement in "Open Items" until the operator
decides.

## Open Questions

1. Is `podcastindex` the name of the mode?
2. Does a schedule of the operator start the pass, as for `import`? Or does the
   mode wait 24 hours between passes in one process?
3. Is 7 days the correct wait in §5?
4. Are the limits of §6 correct: 1,000 rows, 20,000 rows and 2,000
   candidates?
5. Is 28 days the correct length of the measurement in §10? Which count is
   "almost none"?

## Consequences

- A new music feed in PodcastIndex comes into the index within one day, also
  from a host that sends no podping. On 2026-10-31, a pass would have found the
  24 Wavlake albums and the 7 older feeds of the Context.
- The pass sends one API request and approximately 10 MB each day. A candidate
  that fails costs at most one fetch in 7 days. On 2026-10-31, 142 candidates
  failed, so the tries again cost approximately 20 fetches each day.
- The operator registers an API key with PodcastIndex, and keeps the key and
  the secret in the environment of the mode.
- The mode depends on an API that PodcastIndex can change. §6 stops a pass that
  gets a list that is not normal.
- The change spans two repositories. This repository holds the ADR. The
  `stophammer-crawler` commit names it.
- `stophammer-crawler` adds the `sha1` crate of RustCrypto for the
  `Authorization` header. The crate already uses `sha2` of the same family.
- The mode has a `--dry-run` flag. It does the comparison and the log of §8,
  and sends no URL to the pipeline. The first production run uses it.
- A task packet in `docs/tasks/` gives the build after the operator answers the
  open questions.
- `import` keeps its schedule until the measurement of §10 is complete.

## Verification

Mechanical. Each criterion is a test in `stophammer-crawler`:

- The `Authorization` header of fixed inputs is the lowercase hex SHA-1 of the
  key, the secret and the date.
- The `Debug` text and each error text of the credentials hold neither the key
  nor the secret.
- A row with a `podcastGuid` equal to a node `feed_guid` in other letter case
  is held.
- A row at `wavlake.com/feed/<uuid>` with the GUID of a node record at
  `wavlake.com/feed/music/<uuid>` is held.
- A row that matches only by URL, after the removal of the scheme, `www.` and
  a trailing slash, is held.
- The pipeline gets only URLs. Its input type holds no field of the API row.
- A failed page of the node list stops the pass with no URL sent.
- Each condition of §6 stops the pass with no URL sent.
- A candidate with a failed try less than 7 days old is not sent. A candidate
  with a failed try 7 days old or older is sent.
- An `accepted` row of `import_feed_memory` after the previous pass, with an ID
  that is not in the list, is counted. A row with an ID in the list, a row
  with another outcome, and a row from before the previous pass are not
  counted.

Manual. The first pass in production gives the numbers of §8. The operator
compares them with the Context: approximately 8,600 list rows, and a number of
candidates near the 142 that failed. A test cannot make this check, because
it needs the real API and the real index.
