# ADR 0104: A Moved Feed Is Found Without A Podping

## Status
Accepted on 2026-11-17. The operator selected options A and B of the
proposal, and asked that the crawler avoids `429` answers of feed hosts (§6).
Release 0.21.1 holds it. "Open Questions" gives the values that the operator
did not set.

- Bar: crosses repositories. This repository holds the schedule and the
  runbook, and `stophammer-crawler` holds the report of §2.
- Changes: [ADR 0081](0081-a-daily-pass-reads-the-podcastindex-music-list.md)
  §4, §7, §8 and §9
- Uses: ADR 0052 §1 and §2 for the move, ADR 0079 §3 for the release of a
  hold.

## Date
2026-11-17

## Context

In 2026, RSSBlue stopped its feed host. `feeds.rssblue.com` answers `410 Gone`,
and ADR 0067 retired the records. The publishers moved many albums to
`epochradio.com` with the same `podcast:guid`.

ADR 0052 §1 lets only the source URL start a move. Thus the node finds a move
only when these two conditions are both true:

1. The old feed declares the new URL by a `301` or `308` redirect,
   `itunes:new-feed-url` or its self link.
2. A crawler reads the old feed while its host still answers.

In September 2026, 129 albums met both conditions. Their RSSBlue self links
named Epoch Radio. Two albums, "Free" and "Falling Up", did not declare the
new URL. On 2026-11-17, the operator moved them with
`PATCH /v1/feeds/{guid}`.

Each crawler path has a gap for a move:

| Path | Result for a moved feed |
|---|---|
| A podping for the new URL | The node records `feed_url_observed` or `feed_copy_observed` (ADR 0051, ADR 0058). The record does not move. |
| A podping for the old URL | The crawler reads the old feed, and a declared move applies. |
| The `podcastindex` pass | A row with the GUID of a node feed is held (ADR 0081 §4). The pass does not read it, also when its URL is different. |
| `refresh` (ADR 0047) | The crawler reads each old feed, and a declared move applies. No timer starts `refresh`. |
| After the retirement | A body for the GUID from another URL gets `retired_source_conflict` (ADR 0079 §2). |

Thus a host that sends no podping can move a feed, and the node does not see
the move.

PodcastIndex keeps a URL for each GUID. When a publisher moves a feed,
PodcastIndex can have the new URL before the node has it. On 2026-11-17, no
probe confirmed that PodcastIndex had the Epoch Radio URL for the RSSBlue
GUIDs, because the probe needs the API key.

For 927 Wavlake records, the node holds `wavlake.com/feed/music/<uuid>`, and
PodcastIndex holds `wavlake.com/feed/<uuid>` (ADR 0081 Context). These rows
have the same GUID and a different URL, but they are not moves. They have the
same host.

## Decision

### 1. A scheduled `refresh` reads each node feed one time each week

The operator starts `refresh` (ADR 0047) at an interval of 7 days. The pass
reads each old feed while its host answers, so the node finds each declared
move within 7 days. This adds no rule to the move. ADR 0052 §1 stays the only
way that a source starts a move.

The repository ships the systemd units `stophammer-refresh.service` and
`stophammer-refresh.timer`. For Compose, the operator runs the mode in the
service `stophammer-crawler`. `docs/operations.md` gives the command.

### 2. The `podcastindex` pass reports a URL on another host

This section changes ADR 0081 §4. A held row is a **URL difference** when all
of these conditions are true:

- The row is held by its `podcastGuid`.
- The `url` of the row and the `feed_url` of the node feed with that GUID are
  not equal after the comparison of ADR 0081 §4.
- The two hosts are not equal. The host is the part of the compared URL before
  the first `/`, in lowercase.

A URL difference stays a held row. The pass does not fetch its URL, and it
sends nothing to the pipeline or to the node for it. The pass does not move a
feed and does not release a hold. ADR 0052 "Alternatives Considered" rejects a
move from a new URL when the old URL is down.

For each URL difference, the pass writes one log line with the PodcastIndex
ID, the GUID, the node URL and the list URL.

### 3. The pass keeps the URL differences of the last pass

This section changes ADR 0081 §7. The table `music_list_url_differences` of
`import_state.db` holds one row for each URL difference of the last pass. A
row holds the PodcastIndex ID, the GUID, the node URL, the list URL and the
time of the pass.

Each pass replaces all rows of the table in one transaction. A dry run also
replaces them, because the table is a result of the comparison, not of the
pipeline. A pass that stops under ADR 0081 §6, or at a failed node page, does
not change the table.

The table holds no other field of the API row. It holds no row that is not a
URL difference.

### 4. The report line gives the count

This section changes ADR 0081 §8. The report line of the pass also gives the
number of URL differences as `url_differences=N`.

### 5. The operator decides each move

The operator reads the table after a pass with a new count. For each row, the
operator does these checks:

1. The list URL gives a feed with the same `podcast:guid`.
2. The node URL answers gone, or it names the list URL.
3. The publisher, or the new host, confirms the move.

When each check passes, the operator moves the record with
`PATCH /v1/feeds/{guid}` (ADR 0052 §2).

A retired record is not in the node feed list (ADR 0067 §3). Thus its list
row is a candidate, not a URL difference. The pipeline fetch of the row gets
`retired_source_conflict` (ADR 0079 §2), and `music_list_attempts` keeps that
reason. The operator reads these rows also. When the checks pass, the
operator releases the hold (ADR 0079 §3), and the next fetch admits the feed.

`docs/operations.md` gives the commands.

ADR 0081 §9 "Changes to held feeds" now names this report and §1 of this ADR.

### 6. A host that answers `429` gets a longer delay

The weekly `refresh` fetches each node feed. Thus the crawler must make fewer requests
when a host answers `429 Too Many Requests`. This rule applies to each fetch
that uses the host throttle of `stophammer-crawler`. These are the fetches of
`feed`, `refresh` and `podcastindex`, and the follow fetches of `gossip`.

Before this ADR, the crawler made one fetch at a time for each host, with a
delay of at least `HOST_DELAY_MS` (1,500 ms by default). After a `429`, it
waited the `Retry-After` seconds, or 1 s and then 2 s. The next URL of the
host went back to the usual delay. `gossip` did not use `Retry-After`.

Now the throttle keeps a backoff for each host:

1. A fetch that gets `429` doubles the backoff of its host. The first backoff
   is two times `HOST_DELAY_MS`. The backoff is at most 60 s.
2. The next fetch to the host waits for the longest of these three values:
   `HOST_DELAY_MS`, the backoff, and the `Retry-After` of the `429`.
3. The crawler reads `Retry-After` as seconds or as an HTTP date. A value of
   more than 600 s counts as 600 s.
4. After 20 fetches to the host with no `429`, the backoff falls to one half.
   A backoff of `HOST_DELAY_MS` or less becomes zero.
5. Each increase of a backoff writes one log line with the host and the new
   delay.

A failed fetch that the crawler tries again keeps its earlier wait: the
`Retry-After` of the answer, or 1 s and then 2 s. A `Retry-After` of the node
does not change the delay of the feed host.

These limits stay:

- The backoff stays in the memory of one process. Two processes that fetch
  from the same host at the same time do not share it.
- A `gossip` fetch after a podping does not use the host throttle. This was
  true before this ADR. Podpings come in small numbers for each host.
- The throttle uses the host of the requested URL. After a redirect, a `429`
  of the target host increases the backoff of the first host.

## Consequences

- A declared move reaches the node within 7 days, also from a host that sends
  no podping. The source must declare the move before its host stops.
- A move that the source does not declare comes to the operator within 3
  hours, when PodcastIndex has the new URL. The node does not move the feed
  without the operator.
- A move on the same host is not reported. A redirect on that host is the
  usual trigger for such a move, and `refresh` finds it.
- One `refresh` pass fetches each node feed. On the first pass with the follow
  waves, the Wavlake albums took approximately 2 hours 46 minutes at the
  default host delay (`docs/operations.md`).
- The table holds a URL of the API response. The table keeps only the rows of
  the last pass, so the mode builds no database of the list (ADR 0081
  Context, terms of service §5.5(1)).
- A pass of about 10,800 fetches got 2 `429` answers on 2026-09-27
  (`docs/reviews/adr-0050-second-pass-measurement.md`). The backoff of §6
  slows a host only after it answers `429`, so a pass at that rate keeps its
  duration.
- The change spans two repositories. The `stophammer-crawler` commit names
  this ADR.

## Verification

Mechanical. Each criterion is a test in `stophammer-crawler`:

- A row held by its GUID, with a URL on another host, is a URL difference.
- A row held by its GUID, with a URL on the same host but another path, is
  not a URL difference. The Wavlake case of the Context is the test input.
- A row held by its GUID, with a URL that differs only in the scheme, `www.`
  or a trailing slash, is not a URL difference.
- A row held only by its URL is not a URL difference.
- A URL difference is not a candidate, and the pipeline does not get its URL.
- A pass replaces the rows of `music_list_url_differences`. A row of an
  earlier pass that is not a URL difference now is not in the table.
- A dry run writes the table.
- A pass that stops under ADR 0081 §6 does not change the table.
- The report line gives `url_differences=N`.

- A `429` doubles the backoff of its host, from two times the host delay, to
  at most 60 s. The backoff of another host does not change.
- The next fetch to the host waits for the longest of the host delay, the
  backoff and `Retry-After`.
- `Retry-After` in seconds and as an HTTP date gives the wait. A value of more
  than 600 s gives 600 s.
- 20 fetches with no `429` halve the backoff. A backoff of the host delay or
  less becomes zero.
- A retried fetch with `Retry-After: 120` and the status `503` holds the host
  for 120 s and sets no backoff.
- The follow fetches of `gossip` use the same backoff.

Manual. The first pass in production gives the count. The operator reads the
rows and compares them with the RSSBlue records of the Context. A test cannot
make this check, because it needs the real API and the real index.

## Open Questions

The operator did not set these values. The ADR uses the recommended value. The
operator can change each value with no new ADR:

1. The interval of `refresh` is 7 days. The timer starts it on Sunday at
   03:00 in the local time of the host. On the primary host, this is 3 hours
   after the start of the `import` passes.
2. The backoff of §6 starts at two times `HOST_DELAY_MS`, is at most 60 s,
   and halves after 20 fetches with no `429`. A `Retry-After` of more than
   600 s counts as 600 s.
3. The pass does not fetch the list URL of a URL difference. A later change can
   add a fetch, if the operator wants the node to record a copy before the
   move.
