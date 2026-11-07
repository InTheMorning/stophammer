# Research Request: Lookalike Feeds And First-Seen Times

## Status

Received - 2026-11-03. This request comes from the research repository at
`~/build/musicindex-research`. That repository has no Git remote. Its index is
`AGENTS.md`. The note is
`research_notes/aggregation/podcastindex-lookalike-feeds.md` in that
repository.

This document binds nothing in Stophammer. It asks Stophammer to examine four
changes and to make a decision. Stophammer records each decision in its own
ADR.

The data is in `sources/captures/2026-11-03-podcastindex-lookalikes/` in the
research repository. `SHA256SUMS` in that folder gives the hashes. The scripts
are in `tools/podcastindex-lookalikes/` in that repository.

The research examined Stophammer at commit `a34be18`, the crawler at commit
`cb893cb`, the backup `backups/stophammer-20261031T173220Z.db`, and the
PodcastIndex snapshot `podcastindex_feeds.db` of 2026-10-31.

On 2026-11-03 the operator decided that the license of the PodcastIndex data
is not a blocker. PodcastIndex makes this data for projects like this one.

## Purpose

| # | Change | Repository | Evidence |
|---|---|---|---|
| L1 | Read the PodcastIndex lookalike pairs as an input for the operator, not as a decision | stophammer-crawler and stophammer | 99,883 pairs, 23 with a node URL |
| L2 | Find a copy that declares a different `podcast:guid` | stophammer | The "Damn Your Idols" pair |
| L3 | Put the two feeds of a copy in sequence by the PodcastIndex first-seen times | stophammer | 8 of 11 copy rows have the opposite sequence |
| L4 | Compare the payment routes of the two feeds of a pair | stophammer | The detector does not read `podcast:value` |

## What PodcastIndex Publishes

PodcastIndex publishes the result of a detector of lookalike feeds on
`public.podcastindex.org`:

- `lookalike_feeds_all.csv`: all known pairs. 99,883 rows and 36 columns on
  2026-11-03.
- `lookalike_feeds_daily.csv` and `lookalike_feeds_YYYY-MM-DD.csv`: the pairs
  of one day. 62 pairs on 2026-11-03.
- `lookalike_feeds_backlog.html`: the method and the labels.

A pair is two different feeds that carry the same episodes. The detector
matches the item GUIDs. When the item GUIDs are different, it matches three
values: the title, the publication time and the length. The times must be
less than 48 hours apart, and the lengths less than 10 seconds apart.

Each pair has one label: `copy`, `host_migration`,
`same_owner_mirror`, `duplicate_listing` or `unclear`. Each pair also has a
confidence and the signals that fired. The column `original_basis` tells why
feed A is the likely original. The page says that the labels are "likely
classifications for you to verify, not determinations".

The PodcastIndex API specification 1.12.1 gives three first-seen times:

| Field | Object | In the snapshot |
|---|---|---|
| `createdOn`: "The first time the feed was seen by Podcast Index" | Feed | Yes |
| `dateCrawled`: "The time this episode was found in the feed" | Episode | No |
| `valueCreatedOn`: "The time this feed's `value` data added" | Feed | No |

The crawler reads only `id`, `url` and `podcastGuid` from the music list
(`stophammer-crawler/src/podcastindex_api.rs`, `parse_music_list`).

## L1: Read The Pairs As An Input

ADR 0058 records a copy only when a second URL declares the same
`podcast:guid` (`record_feed_copy_for_ingest` in `src/api.rs`). The tests of
ADR 0058 own that rule. The lookalike data compares episodes, so it can
show copies that this rule cannot see.

Of the 99,883 pairs, 23 include a URL in `feeds.feed_url` or
`feed_url_observations.url`. In 9 pairs, the node has the URL of each feed.
The labels are 9 `same_owner_mirror`, 8 `copy` and 6 `host_migration`.

The data has noise. Seven pairs are the two Wavlake URL forms of one feed,
`wavlake.com/feed/<id>` and `wavlake.com/feed/music/<id>`. Three of them have
the label `copy` at confidence 0.60. ADR 0058 calls these URLs aliases.

The proposal:

- The crawler reads `lookalike_feeds_daily.csv` in the pattern of the ADR
  0081 pass.
- The node keeps a pair only when one of its URLs is a node URL.
- The node ignores a pair when its alias rules connect the two URLs.
- The operator copy queue shows the pair with its label, confidence, signals
  and `original_basis`.
- The node does not merge, retire or move a record because of a label.

## L2: A Copy With A Different GUID

"Damn Your Idols" on `feeds.fountain.fm`
(`d64460ea-05e3-5c10-91a2-48dc9607cd7f`) and on `cdn.kolomona.com`
(`65dc63c4-508e-5d73-9622-ab576b8e7754`) are two music records in the node.
No `feed_copies` row connects them. PodcastIndex gives the pair the label
`copy` at confidence 0.75, with the signal `matched_by_titles`.

The research cannot tell if the artist publishes the two feeds. The
operator or the artist must make the decision. This pair type has the most
value, because the current rule cannot find it.

## L3: First-Seen Times

The times of the node are weak evidence of the first time that a feed
existed:

- 7,632 of 10,646 feeds have a `created_at` on 2026-04-19, the day of the
  bulk import.
- `feed_copies.first_seen` gives the first time that the node saw the copy
  URL.

The snapshot has 6,993 node feed URLs. PodcastIndex saw 6,920 of them before
the node, with a median lead of 331 days. 11 of the 84 `feed_copies` rows
have a PodcastIndex time for the source and for the copy. The sequence of
PodcastIndex does not agree with the sequence of the node in 8 of the 11
rows.

The rows show two limits:

- **A move.** For "Autumn Rust" and "The Satellite Skirmish", PodcastIndex saw
  the copy URL on `music.behindthesch3m3s.com` in 2024, and the source on
  `serve.podhome.fm` on 2026-02-06. The older URL is the start of a move.
  Thus the older feed is not always the correct source.
- **Feed time and episode time.** For "Live at Rockpile"
  (`7192ec54-3aa2-5c61-987b-51bf75f68568`), `createdOn` gives the MSP URLs
  first, on 2025-12-06 and 2026-04-21, and the Wavlake URL on 2026-05-03. The
  detector selects the Wavlake feed as the original, with `original_basis`
  `first_stored`. `guid_origin` is `true` for the Wavlake URL. The copy row of
  that URL has the resolution `keep_source`.

The proposal:

- The copy queue shows the PodcastIndex `dateCrawled` of the shared episodes,
  and `createdOn` for each feed, with the times of the node.
- The node uses these times as information for the operator. It does not
  select a source only by an earlier time.

## L4: Payment Routes

The detector does not read `podcast:value`. A copy that keeps the audio and
changes only the payment routes gets the label `same_owner_mirror` or `copy`
with no payment signal. The node has the routes of each feed.

The proposal:

- For each kept pair, the node compares the feed and track routes of the two
  feeds, as `copy_differences` in `src/db.rs` does for a copy row.
- A pair with the same audio and different routes goes to the top of the
  queue. `valueCreatedOn` can show which feed had routes first.

## Data For L2 And L4 - 2026-11-07

Stophammer added this data after the request. It states no decision.

The search `q=haleen&sort=name&type=feed` gives pairs of feeds with one title and
one artist, and different GUIDs. A read-only query examined the backup of
2026-11-07 20:56 UTC with `immutable=1`. It also read the public bodies of one pair,
and it searched the lookalike capture of 2026-11-03.

**Method.** A key is the `release_artist` and the `title` of a music feed, trimmed,
with no case difference. Two feeds with one key are a pair. A group of three feeds
gives three pairs. The index holds 125 such keys, 150 pairs and 42 artists.

| Class | Pairs | Artists | Largest artists |
|---|---|---|---|
| A: other host, same track titles, lengths within 2 s, no shared route address | 109 | 20 | Haleen 40, Prosperous Soul 30, Steered 8, Under The Mulga Groove 7 |
| C: same host, same track titles, lengths within 2 s | 6 | 4 | Sovereign Frequency 3 |
| D: the track titles or the lengths differ | 28 | 21 | Prosperous Soul 5 |
| E: test feeds | 7 | 2 | Test Artist 6 |

**Limits.**

- The match is exact. A different spelling of a title or an artist is not counted,
  so the counts can be low.
- The query compared no audio. Class A is an inference from titles and lengths.
- `feed_copies` has no row for any of the 150 pairs.

**Haleen (L2).** Haleen has 98 music feeds and 58 titles. 40 titles have one feed on
`feeds.fountain.fm` and one on `wavlake.com`.

- 97 of the 98 GUIDs are the UUIDv5 of the URL of the feed. Each of the two bodies
  that the query read declares its GUID. So ADR 0058 cannot join these feeds.
- The item GUIDs are different. In the three pairs that the query read in detail, the
  audio files and the descriptions are also different.
- The Wavlake item is later, with a median of 104 days. It is first in 2 pairs.
- The node holds two publisher feeds with the title Haleen, one on each host.
- This suggests two uploads by the artist. The data does not prove who made each
  upload.

L1 does not find these pairs. One row of 99,885 in the capture holds a URL of a
Haleen music feed, and that row is the "Damn Your Idols" pair of L2. The Fountain
feed of that pair states the author Haleen. The gap between the two uploads is a
possible cause, because the detector needs shared item GUIDs or times less than 48
hours apart. The query did not test this for each pair.

**Routes (L4).** No Haleen pair shares a route address. The Fountain feeds pay
`haleen@fountain.fm` and `boostbot@fountain.fm`. The Wavlake feeds pay one node
address, and each of the 7,738 Wavlake music feeds uses that address. The address
does not show if the payment reaches the same person.

**No merge or hide is proposed.** The routes differ, so a selection of one feed
decides who gets paid.

## Open Questions

- Is a pair of class A a defect, or a correct listing of two uploads? The operator
  has not decided. Nothing is built for this question.
- Does `dateCrawled` keep the first time when PodcastIndex adds a feed again
  with a new ID? The "Live at Rockpile" pair suggests this. The research did
  not examine the API.
- How long does PodcastIndex keep the daily files, and is the column set
  stable?
- Does the "Damn Your Idols" artist publish the two feeds?
- Does the `bymedium` response give `createdOn`? The research did not examine
  it.
