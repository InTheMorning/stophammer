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

## Step 1: A Comment In The Namespace Discussion

Kolomona proposed `rel` on `<podcast:remoteItem>` on 2026-05-21, in the
namespace discussion on publisher feeds:
[#579](https://github.com/Podcastindex-org/podcast-namespace/discussions/579#discussioncomment-17006145).
On 2026-05-25 Kolomona gave a list of values, and on 2026-05-26 proposed
[a comma-separated list](https://github.com/Podcastindex-org/podcast-namespace/discussions/579#discussioncomment-17063987).
matthewruzzi replied that HTML `rel` uses spaces. ChadFarrow, the author of
MSP-2.0, supports the work on the tag.

The proposal thus exists. The operator does not open a new discussion. The
operator posted this text as
[a comment in #579](https://github.com/Podcastindex-org/podcast-namespace/discussions/579#discussioncomment-18626492)
on 2026-09-27.

---

musicindex.org reads `rel` on both sides of a publisher link since
2026-09-24. Some facts from the index, on 2026-09-26:

- It holds 1,772 publisher feeds and 8,249 listed album links.
- In a sample of 249 links, no side of any link stated a role. Most publisher
  feeds are Wavlake artist feeds, with no `rel`.
- Two feeds write `rel`: Sir Libre Records (`rel="label"`) and Jimmy V
  (`rel="artist"`, and `rel="producer"` on an album of another artist).
- No report was found of a client that fails on the attribute.

Three points from our implementation:

1. **When the attribute is absent, the role is not stated.** Our API marks
   such a role as a default, and does not show "artist" as a fact. We ask
   that the specification say the same.
2. **Each side states the role.** When the publisher feed and the album give
   different values, we show the difference and do not select one.
3. **We separate roles with a comma.** A role of two words, such as "sound
   engineer", needs no special spelling with a comma. The one feed with more
   than one role uses commas. Each role is trimmed and lowercased. We
   can change this if the discussion selects spaces.

---

## Step 2: The MSP-2.0 Change

This is item 1 of [the MSP-2.0 todo list](msp-2.0-todo.md), which holds each
change for MSP-2.0.

The operator opened this issue as
[#148](https://github.com/ChadFarrow/MSP-2.0/issues/148) in
`ChadFarrow/MSP-2.0` on 2026-09-27, and can offer a pull request. The facts are from commit `66f0d53` of 2026-09-27.

---

**Write the publisher role as `rel` on both sides of a publisher link**

MSP-2.0 writes both sides of a publisher link. `generateRemoteItemXml` writes
each listed feed of a publisher feed, and `generatePublisherXml` writes the
`<podcast:publisher>` reference of an album. The publish flow in
`src/utils/publisherPublish.ts` adds that reference to each catalog feed, and
`PublishSection.tsx` turns this option on by default. Thus each MSP publisher
link is two-way. That is the best case for an index, because both feeds agree
on the link.

No field holds the role of the publisher. An MSP feed cannot say if the
publisher is the artist, a label, a network or a producer. An app that shows a
publisher page must guess, and most apps show each publisher as an artist.

**Change**

1. Add `rel?: string` to `RemoteItem` and to `PublisherReference` in
   `src/types/feed.ts`.
2. `src/utils/xmlParser.ts` reads the `rel` attribute of each
   `<podcast:remoteItem>`, and of the `remoteItem` inside
   `<podcast:publisher>`. An imported feed then keeps its value.
3. `src/utils/xmlGenerator.ts` writes `rel="…"` in `generateRemoteItemXml` and
   in `generatePublisherXml` when the field has a value. It writes no attribute
   when the field is empty.
4. The publisher editor gets one control: "This publisher is: Not stated,
   Artist, Label, Network, Producer". The default is "Not stated". The value
   applies to each catalog feed, and a user can change it for one catalog
   feed.
5. The publish flow writes the same value into the `PublisherReference` of
   each catalog feed that it updates, so that the two sides agree.
6. Tests: a round trip for each value, and a test that an empty value writes
   no attribute.

**Example, a label**

```xml
<!-- In the publisher feed -->
<podcast:remoteItem feedGuid="917393e3-1b1e-5cef-ace4-edaa54e1f810"
    feedUrl="https://example.com/album.xml" medium="music" rel="label" />

<!-- In the album feed -->
<podcast:publisher>
    <podcast:remoteItem medium="publisher" rel="label"
        feedGuid="003af0a0-6a45-55cf-b765-68e3d349551a"
        feedUrl="https://example.com/label.xml" />
</podcast:publisher>
```

More than one role is a comma-separated list, for example
`rel="artist,label"`.

**Status of the attribute**

`rel` on `<podcast:remoteItem>` is not in the specification yet. Kolomona
proposed it in the namespace discussion on publisher feeds:
[#579](https://github.com/Podcastindex-org/podcast-namespace/discussions/579#discussioncomment-17006145),
with the list of values in
[a later comment](https://github.com/Podcastindex-org/podcast-namespace/discussions/579#discussioncomment-17063987).
The separator is still open there: a comma or a space.

Two music indexes read it
now: v4vmusic.com and musicindex.org. Two feeds write it now: Sir Libre Records
(`rel="label"`) and Jimmy V (`rel="artist"` and `rel="producer"`). No client
failure is known. MSP already writes the non-standard `feedImg` attribute for
the same reason: a parser ignores an attribute that it does not know.

When the attribute is absent, the role is not stated. MSP writes no default
value, so that an index does not show a guess as a fact.

I can send a pull request for this change.

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

## Step 4: Fountain

Fountain is the second host of music feeds in the index. On the production
copy of 2026-09-26 it held 330 albums and 70 publisher feeds. Each of the 330
albums names its publisher, and the 70 publisher feeds list 329 links. Thus
Fountain writes both sides of each link, and only the role is missing. No
Fountain link states a `rel`.

The operator brings the change to the developers of Fountain, who are open to
it. Fountain is the first step, because one change states the role of about
330 links, and a second implementation helps the discussion #579. The publisher
chooses "Not stated", "Artist" or "Label", and Fountain writes the same `rel`
value on both sides of each link. "Not stated" writes no attribute.

The request links to the namespace discussion
[#579](https://github.com/Podcastindex-org/podcast-namespace/discussions/579).
It does not link to MSP-2.0 issue #148, which has no answer. It tells that the
index reads a comma or a space as the separator (ADR 0049 §6).

## Step 5: Wavlake

Wavlake hosts 7,531 albums and 1,619 publisher feeds, each album with a link
to its publisher. A `rel="artist"` on its artist feeds would state the role of
most publishers in the index. The operator expects no answer from Wavlake, so
no other step depends on this one. The operator asks after the namespace
accepts `rel`.

## Measurement

After each step, repeat the count of `role_source` over all publisher links.
The baseline of 2026-09-26 has 0 stated roles in a sample of 249 links. Sir
Libre Records has 7 links with a stated role.
