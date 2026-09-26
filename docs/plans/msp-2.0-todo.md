# MSP-2.0 Todo List

Date: 2026-09-26. This list states no rule for Stophammer. It holds the
changes that the operator brings to MSP-2.0
([ChadFarrow/MSP-2.0](https://github.com/ChadFarrow/MSP-2.0)). With them, the
feeds that MSP writes give this index the facts it needs. Each item names its
evidence and the Stophammer decision that reads the fact.

The facts about MSP come from commit `183e424` of 2026-08-30.

## Status

| # | Item | Stophammer owner | Waits for | Sent |
|---|---|---|---|---|
| 1 | Write the publisher role as `rel` | ADR 0049 §6 | The namespace proposal of `rel` | No |
| 2 | A visibility control for the block tags | ADR 0057 | Nothing | No |
| 3 | A playlist feed (`musicL`) | ADR 0060 | Nothing | No |
| 4 | A warning before a GUID change of a published feed | ADR 0052 §5 | Nothing | No |

## 1. Write The Publisher Role As `rel`

**Now.** MSP writes both sides of a publisher link. `generateRemoteItemXml`
writes each listed feed of a publisher feed, and `generatePublisherXml` writes
the `<podcast:publisher>` of an album. The publish flow adds the publisher
reference to each catalog feed, and the option is on by default. No field holds
the role, so no MSP feed can say if the publisher is the artist or a label.

**Change.** The text of the issue is step 2 of
[the adoption plan](publisher-rel-adoption-plan.md):

- `rel?: string` on `RemoteItem` and `PublisherReference`.
- The parser reads `rel`, and the generator writes it when it has a value.
- One control in the publisher editor: "This publisher is: Not stated,
  Artist, Label, Network, Producer". The default is "Not stated".
- The publish flow writes the same value into each catalog feed.

**Waits for** the namespace discussion of step 1 of the adoption plan. The
issue links to it.

## 2. A Visibility Control For The Block Tags

**Now.** MSP writes neither `itunes:block` nor `podcast:block`. A musician on
MSP cannot leave a directory through the feed.

**Evidence.** On 2026-09-26 the operator tested Podcast Index: it acts on
`itunes:block` `yes`, and not on `podcast:block`. This index acts on
`podcast:block`, and not on `itunes:block` (ADR 0057). Thus a musician needs
both tags to say each wish.

**Change.** One "Visibility" control in the album editor and in the publisher
editor. Each choice writes these channel tags:

| Choice | Tags |
|---|---|
| Listed everywhere (default) | No block tag |
| Not in podcast apps | `<itunes:block>Yes</itunes:block>` |
| Not in musicindex | `<podcast:block id="musicindex">yes</podcast:block>` |
| Not listed anywhere | `<itunes:block>Yes</itunes:block>` and `<podcast:block>yes</podcast:block>` |
| Only in musicindex | The two tags above, and `<podcast:block id="musicindex">no</podcast:block>` |

The default writes no tag. The tool Sovereign Feeds writes
`<podcast:block>no</podcast:block>` on each feed. That tag names no service, so
it has no effect, and MSP should not copy it.

## 3. A Playlist Feed (`musicL`)

**Now.** The feed types of MSP are `album`, `video` and `publisher`
(`FeedType` in `src/types/feed.ts`). MSP cannot make a playlist. Its
`RemoteItem` type already has `itemGuid` and `title`.

**Evidence.** On 2026-09-26 this index held 6 track playlists. Two of them,
from Kolomona, give no `feedUrl` on any entry: 278 and 383 entries. The crawler
cannot follow such an entry to its album (ADR 0060 §5). 16 album GUIDs of those
two playlists stay missing from the index.

**Change.** A playlist feed type:

- `<podcast:medium>musicL</podcast:medium>` in the channel, and no `<item>`.
- One channel `<podcast:remoteItem>` for each track, with `medium="music"`,
  `feedGuid`, `feedUrl`, `itemGuid` and `title`. `feedUrl` is always written.
- An optional `podcast:value` block for the author of the playlist. This index
  keeps it as source data (ADR 0060 §4).

## 4. A Warning Before A GUID Change Of A Published Feed

**Now.** `regenerateAlbumGuids` in `src/utils/regenerateGuids.ts` gives a new
feed GUID and new track GUIDs. MSP calls it only in the "duplicate this feed"
flow, which is correct.

**Evidence.** On 2026-09-25 another tool, not MSP, changed the GUIDs of four
published Doerfelverse albums. Each change is now pending in
`GET /v1/guid-changes` (ADR 0052 §5), and the publisher confirmed that the tool
made an error.

**Change.** A user can edit the `podcast:guid` of a feed that MSP already
hosts, or of an imported feed. Before the change, MSP shows a warning. Indexes
treat a new GUID as a new show, and the old show loses its listing. The duplicate flow does not
change.

## Keep As It Is

These MSP behaviors are correct for this index. A change to them would harm the
index:

- Both sides of each publisher link, with the catalog update on by default.
  The one-way links in this index come from other tools: RSS Blue, The Split
  Kit and Sovereign Feeds.
- `<atom:link rel="self">` in each feed. ADR 0052 moves a record on it.
- `title` as an attribute of `<podcast:remoteItem>`.
- A podping with `medium` set, from `notifyPodping`.
- New GUIDs only for a duplicate.
