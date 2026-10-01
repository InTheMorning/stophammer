# MSP-2.0 Todo List

Date: 2026-09-26. This list states no rule for Stophammer. It holds the
changes that the operator brings to MSP-2.0
([ChadFarrow/MSP-2.0](https://github.com/ChadFarrow/MSP-2.0)). With them, the
feeds that MSP writes give this index the facts it needs. Each item names its
evidence and the Stophammer decision that reads the fact.

The facts about MSP come from commit `66f0d53` of 2026-09-27. They were
checked again on 2026-10-01.

## How To Send A Change

MSP-2.0 has one maintainer. On 2026-10-01 the repository had 100 pull requests,
each by the maintainer, and 11 issues. Ten issues were notes of the maintainer,
with no comments. Issue #148 had no answer after 4 days. Thus an issue alone
probably does not reach the maintainer. The operator asks the maintainer
directly first, then sends a small pull request.

## Status

| # | Item | Stophammer owner | Waits for | Sent |
|---|---|---|---|---|
| 1 | Write the publisher role as `rel` | ADR 0049 §6 | Nothing. Done | [#149](https://github.com/ChadFarrow/MSP-2.0/pull/149), merged on 2026-10-01 as `8179257` |
| 2 | A listing control: listed, hidden from podcast directories, hidden everywhere | ADR 0057 | The review of the pull request | [#150](https://github.com/ChadFarrow/MSP-2.0/pull/150), opened on 2026-10-01 |
| 3 | A playlist feed (`musicL`) | ADR 0060 | Item 1 | No. A feature idea, not a defect |

## 1. Write The Publisher Role As `rel`

**Now.** MSP writes both sides of a publisher link. `generateRemoteItemXml`
writes each listed feed of a publisher feed, and `generatePublisherXml` writes
the `<podcast:publisher>` of an album. The publish flow adds the publisher
reference to each catalog feed, and the option is on by default. No field holds
the role, so no MSP feed can say if the publisher is the artist or a label.

**Change.** The text of the issue is step 2 of
[the adoption plan](publisher-rel-adoption-plan.md). On 2026-10-01 the request
became smaller, because a small change for a tag that is not in the
specification is easier to accept:

- `rel?: string` on `RemoteItem` and `PublisherReference`.
- The parser reads `rel`, and the generator writes it when it has a value.
- One control in the publisher editor: "Not stated", "Artist" or "Label". The
  default is "Not stated", and it writes no attribute.
- The publish flow writes the same value into each catalog feed. There is no
  choice for one catalog feed.

On 2026-10-01 the maintainer welcomed a pull request and merged it as
[#149](https://github.com/ChadFarrow/MSP-2.0/pull/149). The pull request also
copies the `rel` of each catalog entry into the `<podcast:publisher>` of its
album. It fixes a loss: a parse and regenerate of a feed with `rel` dropped
the attribute.

**Waits for** nothing. Kolomona proposed `rel` in the namespace discussion
[#579](https://github.com/Podcastindex-org/podcast-namespace/discussions/579#discussioncomment-17006145),
and the issue links to it. Step 1 of the adoption plan is a comment in that
discussion.

## 2. A Listing Control

**Now.** MSP writes neither `itunes:block` nor `podcast:block`. A musician on
MSP cannot leave a directory through the feed.

**Evidence.** On 2026-09-26 the operator tested Podcast Index: it acts on
`itunes:block` `yes`, and not on `podcast:block`. This index acts on
`podcast:block`, and not on `itunes:block` (ADR 0057).

**Change.** One "Listing" control in the album editor and in the publisher
editor, with three choices:

| Choice | Tags |
|---|---|
| Listed (default) | No block tag |
| Hidden from podcast directories | `<itunes:block>Yes</itunes:block>` |
| Hidden everywhere | `<itunes:block>Yes</itunes:block>` and `<podcast:block>yes</podcast:block>` |

The control names no index. A choice for musicindex in the UI of a tool of
another person looks like an advertisement, and the maintainer would probably
refuse it. "Hidden everywhere" names no service, so it has no such problem.

**Effect on this index.** "Hidden everywhere" removes the feed from this index,
because its plain `podcast:block` addresses each service. "Hidden from podcast
directories" keeps the feed in this index, because ADR 0057 reads only
`podcast:block`. That is the intent of the choice: a musician can leave the
podcast apps and stay in music apps.

The tool Sovereign Feeds writes `<podcast:block>no</podcast:block>` on each
feed. That tag names no service, so it has no effect, and MSP should not copy
it.

## 3. A Playlist Feed (`musicL`)

**Now.** The feed types of MSP are `album`, `video` and `publisher`
(`FeedType` in `src/types/feed.ts`). MSP cannot make a playlist. Its
`RemoteItem` type already has `itemGuid` and `title`.

**Evidence.** An entry with no `feedUrl` resolves only when the index already
holds its album (ADR 0060 §5). An MSP playlist that always writes `feedUrl`
avoids that limit.

On 2026-09-26 two playlists of Kolomona gave no `feedUrl` on any entry, and 16
of their album GUIDs were not in the index. On 2026-10-01 Kolomona told the
operator that the two playlists come from the value splits of his feeds and of
Adam Curry's feeds. Those splits name albums that no longer exist. No
other feed in the index names any of the 16 GUIDs. Thus a `feedUrl` would not
add them, and no change to those files is necessary. The API gives
`remote_track_guid` null for such an entry, so a client can skip it.

This item is a feature idea for MSP, not a defect, and waits until item 1 has
an answer.

**Change.** A playlist feed type:

- `<podcast:medium>musicL</podcast:medium>` in the channel, and no `<item>`.
- One channel `<podcast:remoteItem>` for each track, with `medium="music"`,
  `feedGuid`, `feedUrl`, `itemGuid` and `title`. `feedUrl` is always written.
- An optional `podcast:value` block for the author of the playlist. This index
  keeps it as source data (ADR 0060 §4).

## Keep As It Is

These MSP behaviors are correct for this index. A change to them would harm the
index:

- Both sides of each publisher link, with the catalog update on by default.
  The one-way links in this index come from other tools: RSS Blue, The Split
  Kit and Sovereign Feeds.
- `<atom:link rel="self">` in each feed. ADR 0052 moves a record on it.
- `title` as an attribute of `<podcast:remoteItem>`.
- A podping with `medium` set, from `notifyPodping`.
- New GUIDs only for a duplicate. The "Generate new GUID" button asks for
  confirmation, with the text "This will create a new feed identity". The
  Doerfelverse GUID change of 2026-09-25 came from another tool.
