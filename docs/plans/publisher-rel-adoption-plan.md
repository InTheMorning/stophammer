# Publisher Role Adoption Plan

Date: 2026-09-26. This plan states no rule for Stophammer. It holds the texts
that the operator sends outside this repository, so that publishers can state
the role of a publisher feed.

## The Problem

A publisher feed (`podcast:medium` `publisher`) lists the albums of an artist
or of a label. No standard attribute says which. On 2026-09-26:

- The index holds 1,772 publisher feeds and 8,249 listed album links.
- In a sample of 249 links, no side of any link stated a role.
- Two feeds state a role with a non-standard `rel` attribute: Sir Libre
  Records (`rel="label"`) and Jimmy V (`rel="artist"` and `rel="producer"`).
- Two indexes read `rel`: v4vmusic.com since 2026-05-25, and this index since
  ADR 0049 §6.
- No report was found of a client that fails on the attribute.

## Step 1: The Namespace Proposal

The operator posts this text as a new discussion in
`Podcastindex-org/podcast-namespace`.

---

**Proposal: an optional `rel` attribute on `<podcast:remoteItem>` for the role
of a publisher**

**Problem.** A feed with `<podcast:medium>publisher</podcast:medium>` lists the
feeds of a "parent publishing entity". That entity can be the artist, a
label, a network or a producer. The specification gives no way to state
which. An app that shows a publisher page must guess. Two music indexes
already read a non-standard `rel` attribute for this purpose, and some feeds
already write it.

**Proposal.** Add one optional attribute to `<podcast:remoteItem>`:

- `rel` (optional): the role of the publishing entity for the feed that the
  link joins. The values are `artist`, `label`, `network` and `producer`. More
  than one role is a comma-separated list, for example `rel="artist,label"`.

The attribute has a meaning only on a link between a publisher feed and a
feed that it publishes:

- In a publisher feed, on each `<podcast:remoteItem>` that lists a feed.
- In a published feed, on the `<podcast:remoteItem medium="publisher">` inside
  `<podcast:publisher>`.

Each side states the role of the publisher. When the two sides give different
values, an app shows the difference. It does not select one.

**When the attribute is absent, the role is not stated.** An app must not
show "artist" as a fact when no side states it.

**Example, a label:**

```xml
<!-- In the publisher feed -->
<podcast:medium>publisher</podcast:medium>
<podcast:remoteItem medium="music" rel="label"
    feedGuid="917393e3-1b1e-5cef-ace4-edaa54e1f810"
    feedUrl="https://example.com/album.xml" />

<!-- In the album feed -->
<podcast:publisher>
    <podcast:remoteItem medium="publisher" rel="label"
        feedGuid="003af0a0-6a45-55cf-b765-68e3d349551a"
        feedUrl="https://example.com/label.xml" />
</podcast:publisher>
```

**Compatibility.** The attribute is optional. An XML parser ignores an
attribute that it does not know. Feeds that write `rel` today are in Podcast
Index with no known failure.

**Questions for the community.**

1. Is a comma the correct separator, or a space, as in HTML `rel`?
2. Is the list of values complete? `distributor` is one more candidate.
3. Is `rel` the correct name? The namespace uses `rel` on
   `<podcast:location>` proposals and on `<podcast:alternateEnclosure>` with
   other meanings.

---

## Step 2: The MSP-2.0 Change

This is item 1 of [the MSP-2.0 todo list](msp-2.0-todo.md), which holds each
change for MSP-2.0.

The operator opens this issue in `ChadFarrow/MSP-2.0`, and can offer a pull
request. The facts are from commit `183e424` of 2026-08-30.

---

**Write the publisher role as `rel` on both sides of a publisher link**

MSP-2.0 writes both sides of a publisher link. `generateRemoteItemXml` writes
each listed feed of a publisher feed, and `generatePublisherXml` writes the
`<podcast:publisher>` reference of an album. The publish flow in
`publisherPublish.ts` adds that reference to each catalog feed. This is the
best case: each MSP publisher link is two-way.

No field holds the role of the publisher, so no MSP feed can state if the
publisher is the artist or a label. Indexes then show every publisher as an
artist.

**Change.**

1. Add `rel?: string` to `RemoteItem` and to `PublisherReference` in
   `src/types/feed.ts`.
2. `xmlParser.ts` reads the `rel` attribute of each `podcast:remoteItem`, so
   an imported feed keeps its value.
3. `xmlGenerator.ts` writes `rel="…"` when the field has a value, in
   `generateRemoteItemXml` and in `generatePublisherXml`. It writes nothing
   when the field is empty.
4. The publisher editor gets one control: "This publisher is: Not stated,
   Artist, Label, Network, Producer". The default is "Not stated". The value
   applies to each catalog feed, and one catalog feed can change it.
5. The publish flow writes the same value into the `PublisherReference` of
   each catalog feed that it updates, so the two sides agree.
6. Tests: a round-trip test for each value, and a test that an empty value
   writes no attribute.

The attribute is not in the specification yet. A namespace proposal is open:
[link to the discussion]. MSP already writes the non-standard `feedImg`
attribute for its own editor, with the same reason: a parser ignores an
attribute that it does not know.

---

## Step 3: This Index

- ADR 0061 gives the artists of the listed albums. It corrects the "0 artists"
  of a publisher with only one-way links.
- The index derives no role. `role_source: "default"` marks each role that no
  feed states.
- The musicindex.org site shows, on a publisher page:
  - "Role not stated" when `role_source` is `default`, with a link to a page
    that tells a publisher how to add `rel`.
  - "Listed only: this album does not name the publisher" when
    `music_names_publisher` is false.
  - "The feeds do not agree" when `role_source` is `conflict`. On 2026-09-26
    Sir Libre Records has 3 such links.

## Step 4: Wavlake

Most of the 1,772 publisher feeds are Wavlake artist feeds. Each is the page of
one artist. When the namespace accepts `rel`, the operator asks Wavlake to
write `rel="artist"` on each link of its artist feeds. One change then states
the role of most publishers in the index.

## Measurement

After each step, repeat the count of `role_source` over all publisher links.
The baseline of 2026-09-26 has 0 stated roles in a sample of 249 links. Sir
Libre Records has 7 links with a stated role.
