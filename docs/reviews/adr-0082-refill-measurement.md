# ADR 0082: Measurement Of The Refill Pass

This record is advisory. It states no binding rule.
[ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md) owns the rules. This
record gives the refill pass in production, the refill check, and the reviews of
the build.

## The Pass

The times are in UTC. The report line of the pass, its `fetch:` line and its
start and end times were not kept. The times below come from the database.

| Fact | Value |
|---|---|
| Migration 0058 applied | 2026-10-31 23:45:36 |
| First funding row written | 2026-10-31 23:46:50 |
| Release 0.18.0 published | 2026-10-31 23:47:54 |
| Last funding row written | 2026-11-01 05:32:34 |
| Time between the first and the last row | 5 hours 46 minutes |

The release was promoted approximately 1 minute after the first row, while the
pass ran. The planned sequence was the check first, and the promotion after it.
The promotion restarted the primary. In the fetch cache, 2 feeds outside the
held set have an `ingest_error` from a failed request to the primary. This
record does not show that the restart caused them.

## The Refill Check

The cache count before the pass was not kept. The count below was made on
2026-11-05 from the fetch cache of that date. So it includes bodies that the
crawler fetched after the pass. The funding counts come from the backup of
2026-11-05, and they did not change between the two backups of that date.

- Feeds with a funding row: 115. Of these, 113 have a feed row and 18 have track
  rows. The total is 220 rows.
- Cache rows: 21,067.
- Cache bodies with the text `podcast:funding`: 429.
- Ratio of the feeds with a row to the cache bodies: 0.27.

The ratio is below the limit of one half in the task of the check. The cache
holds many feeds that the node does not hold, so a second count compared the
two lists:

| Group of cache bodies | Bodies |
|---|---|
| The text is present, and no tag has a usable `url` | 116 |
| A usable tag, and the feed is not held | 186 |
| A usable tag, the feed is held, and the feed has a row | 118 |
| A usable tag, the feed is held, and the feed has no row | 9 |

The 118 bodies match all 115 feeds with a row, because a feed can match more
than one cache row. For the held feeds with a usable tag, 118 of 127 bodies have
a row, which is 93%. The test for a usable tag is a text pattern, and this
record did not examine the 116 bodies in the first group.

The largest reason in the second group is `missing podcast:guid`, with 161
feeds. The node cannot hold a feed with no GUID.

### The 9 Bodies With No Row

Each body was examined on 2026-11-05. None shows a defect of the parser or the
node.

| Feeds | Last body in the cache | Cause |
|---|---|---|
| 7 feeds at `feeds.rssblue.com` | 2026-09-24 and 2026-10-02 | The host now answers `410`. These bodies are older than the pass, so the pass did not get a body. |
| `kolomonas-test-playlist-rss.xml` at `cdn.kolomona.com` | 2026-10-02 | The host answered `403` to a check with a different user agent. The body is older than the pass, so the pass did not get a body. |
| `kill-the-mf-beat.xml` at `www.doerfelverse.com` | 2026-10-31 23:47:53 | The body declares a new GUID. The change is pending in `GET /v1/guid-changes`, and ADR 0052 §5 records that the operator waits for the feed author. |

### The Known Feed

`GET /v1/feeds/8553c853-6a25-4238-840b-1c0a9d4740d2?include=source_links` gives
one `funding` row with the label `Show me some love!`. The backup gives the URL
`https://patreon.com/oystein`, the source `podcast_funding` and the extraction
path `feed.podcast:funding`.

## The Reviews Of The Build

The reviews ran on 2026-10-31, on the working tree before the commit:

| Task | Result | Required fixes |
|---|---|---|
| 001, the parser | Pass | None. The crawler gate also passed against the changed parser: 341 tests, clippy and fmt. |
| 002, the storage and the events | Pass | None. The packet missed the `MIGRATIONS` list, a second insert in `ingest_transaction` and the version assertions of `migration_tests`. The agent changed each one. |
| 003, the response and the documents | Pass, with one fix by the orchestrator | The `docs/API.md` sentence on `entity.podcast:funding` named only a track. It now names a track or a live item, because the feed include lists live item rows too. |

## Open

- The feeds at `feeds.rssblue.com` keep their records with no funding row while
  the host answers `410`. This record makes no decision about them.
- The Elijah Lied feed has no funding row while its GUID change is pending.
