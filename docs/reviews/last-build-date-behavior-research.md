# Research: The Behavior Of `lastBuildDate`

Date: 2026-09-25.

Status: advisory. This record gives evidence for the decision of
[ADR 0053](../adr/0053-a-correction-stays-applied.md) section 3. It states no
rule. ADR 0053 task 005 waits for that decision.

## The Question

ADR 0053 section 3 rejects a submission when its `lastBuildDate` is earlier
than the stored value. Three questions decide if that rule is correct:

1. What does `lastBuildDate` mean for the feeds in this index?
2. Does an honest feed ever move its `lastBuildDate` back, or into the future?
3. How does the rule interact with the repair of ADR 0051 and with the fetch
   cache of ADR 0050?

## History In The Code

| Date | Commit | Change |
|---|---|---|
| 2026-03-12 | parser `08a082c` | The first parser reads `lastBuildDate` into `FeedField::PubDate` as a fallback. The first value wins, so `lastBuildDate` applies only when the channel has no valid `pubDate`. |
| 2026-03 to 2026-09 | node | `pub_date` supplies `release_date`. For most feeds, the value was the feed build time, and its claim label said `feed.pub_date`. |
| 2026-09-22 | ADR 0043, parser `0c8d8cf`, crawler `f28de4d`, node `4cb9adc` | `lastBuildDate` gets its own field. The node stops using it as a release date, stores it in `feeds.last_build_date` (migration 0034), and returns it in the API. The measurement for ADR 0043: 7,095 of 7,538 feeds had `lastBuildDate` and no channel `pubDate`. |
| 2026-09-22 to now | node | The node stores the value and returns it. No code compares two values. |

Before 2026-09-22 the node did not store the value apart. So the stored
`last_build_date` of a feed comes only from ingests after that date.

## Measurements

Three sources:

- The April snapshot, `stophammer-crawler/analysis/data/feed_audit.ndjson`,
  7,538 feeds. Each row holds the fetch time and the raw XML. The script reads
  the first `lastBuildDate` before the first `<item`.
- Two production backups from 2026-09-24, at 00:23 UTC and 20:01 UTC, in
  `backups/`. The queries ran on copies, read-only.
- The script is in the session scratchpad. It is not kept in the repository.

### What the value means, by host

The difference between `lastBuildDate` and the fetch time, in April:

| Host | Feeds | Within 120 s of the fetch | Earlier | Absent |
|---|---:|---:|---:|---:|
| `wavlake.com` | 6,643 | 6,512 | 131 | 0 |
| `rssblue.com` | 286 | 0 | 286 | 0 |
| `fountain.fm` | 163 | 0 | 163 | 0 |
| All others | 446 | 0 | 231 | 215 |

Wavlake writes the request time into `lastBuildDate`. For Wavlake, the value
tells when the body was generated, which is almost the fetch time. For the
other hosts, the value changes only when the publisher changes the feed.

### Does an honest value move back or into the future

| Comparison | Feeds compared | Equal | Later | Earlier |
|---|---:|---:|---:|---:|
| April snapshot to 2026-09-24 20:01 UTC, same URL | 5,990 | 282 | 5,708 | **0** |
| 2026-09-24 00:23 UTC to 20:01 UTC, same URL | 7,778 | 707 | 7,071 | **0** |

| Snapshot | Feeds with a value | More than one day in the future |
|---|---:|---:|
| April snapshot | 7,323 | **0** |
| 2026-09-24 00:23 UTC | 7,778 | **0** |
| 2026-09-24 20:01 UTC | 9,840 | **0** |

No feed moved its value back. No value was in the future.

### The repair and the mirrors

On 2026-09-24 at 20:01 UTC, 1,619 feeds had a URL observation at a URL that is
not their stored URL. 1,407 of those URLs are on Wavlake. Before ADR 0051,
the node applied the body of such a mirror URL to the record. A Wavlake mirror
body carries its own fetch time as `lastBuildDate`.

Thus a record that a mirror changed holds the fetch time of the mirror. The
ADR 0051 repair replays the source body of the last `refresh` pass. In that
pass, the publisher-link wave can fetch a mirror after its source. Then the
stored value is later than the source body, and the rule of section 3 rejects
the repair with `stale_submission`. The backups do not show how many records
are in this condition.

### Two more facts from the data

- A Wavlake body changes on each fetch, because the date changes. The hash
  shortcut of `content_hash` never matches a Wavlake feed. On 2026-09-24 the
  node emitted 2,696 `feed_upserted` events.
- A body that the crawler keeps after a `304` (ADR 0050) is the body of the
  last `200` for that URL. After ADR 0051, that is the body the node holds for
  the record, so its value is equal to the stored value and passes.

## What The Rule Does For Each Host

- **Wavlake.** The rule rejects a body generated before the stored body. That
  covers a CDN copy older than the stored one, and a replay of an old
  snapshot. The CDN answer has `cache-control: max-age=43200`, so a copy can
  be as much as 12 hours old.
- **Hosts with a fixed value.** The rule rejects an earlier edition of the
  feed. That covers a replay and an old host after a move. It also rejects a
  publisher who restores an older file with its older date. That case is not
  in the data.
- **A feed with no value.** The rule does nothing. 215 feeds in April.

## The Threat That Remains After ADR 0051

ADR 0051 stops a mirror from writing a record. The stale copies that remain:

- An operator replay of an old snapshot through the `ndjson` mode. The ADR
  0050 context names this risk. The April snapshot would set 5,708 feeds back.
- A CDN copy older than the stored copy.
- A holder of `CRAWL_TOKEN`. ADR 0055 owns this attacker.

## Options

| Option | Effect | Cost |
|---|---|---|
| A. The rule as written | Stops a replay and an older CDN copy | Blocks the ADR 0051 repair of mirror records unless the repair runs first. Freezes a feed whose stored value is in the future until that time |
| B. Option A, and a value more than 24 hours in the future counts as absent | As option A, and a future value cannot freeze a feed | One comparison. No incident supports it: the data has no future value |
| C. Order by the fetch time that the crawler reports | Same order as A for Wavlake. Also works for a feed with no value | A new trusted field in the ingest contract. The crawler reports it, so a token holder can set it |
| D. No stale rule | Nothing to deploy in order | A replay of an old snapshot rolls feeds back. The operator must not replay without care |

## Recommendation

Option A, with two conditions. The data supports the rule as written, and it
shows no incident for the clamp of option B.

1. **The ADR 0051 repair runs before ADR 0053 is deployed.** Otherwise the
   rule rejects the repair of mirror records.
2. **Before the ADR 0053 deploy, a query shows no stored value in the
   future:**

   ```sql
   SELECT COUNT(*) FROM feeds
   WHERE last_build_date > CAST(strftime('%s','now') AS INTEGER) + 86400;
   ```

   The count must be `0`. If it is not, add the clamp of option B first.

The ADR 0053 deploy task can hold both conditions as steps. ADR 0053 section 3
needs no change for option A.

## Not Verified

- The behavior of hosts after 2026-09-24.
- A publisher that restores an older file. It is not in the data.
- The value that each mirror wrote. The backups hold the current value of each
  record, not the source of that value.
