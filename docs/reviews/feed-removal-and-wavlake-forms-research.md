# Research: Feed Removal By The Publisher, And The Wavlake URL Forms

Date: 2026-09-25.

Status: advisory. This record gives evidence for two decisions. It states no
rule.

- If the public proof flow of ADR 0018 goes offline, can a publisher still
  remove a feed without the operator?
- After ADR 0051, how does the index treat the several URL forms that Wavlake
  serves for one album?

## 1. `podcast:block`

### The specification

The tag is in the channel. A feed can hold more than one. The value is `yes`
or `no`. The optional `id` attribute holds one slug from the service slug
list. The specification says:

> Platforms should not ingest a feed for public display/use if their slug
> exists in the `id` of a `yes` block tag, or if an unbounded `yes` block tag
> exists.

A platform examines the tags in this sequence:

1. `<podcast:block id="[myslug]">no</podcast:block>`: the platform can ingest.
2. `<podcast:block id="[myslug]">yes</podcast:block>`: do not ingest.
3. `<podcast:block>yes</podcast:block>`: do not ingest.

A publisher can block all platforms, block some platforms, or block all
platforms except some.

The service slug list has `podcastindex`. It has no slug for `musicindex` or
`stophammer`. Thus today only an unbounded `yes` applies to this index. A
slug for this index needs a change to the namespace repository.

Sources: [block specification](https://github.com/Podcastindex-org/podcast-namespace/blob/main/docs/tags/block.md),
[service slugs](https://github.com/Podcastindex-org/podcast-namespace/blob/main/serviceslugs.txt),
[the proposal, issue 179](https://github.com/Podcastindex-org/podcast-namespace/issues/179).

### `itunes:block`

`itunes:block` is an Apple tag. It asks Apple not to list a show. A publisher
can set it to leave Apple only, so it does not state a wish for this index.
This record does not recommend it as a signal. How other directories read it
is not verified here.

### The data

The April snapshot, 7,538 feeds, channel level:

| Tag | Feeds |
|---|---:|
| `podcast:block` `yes`, with or without `id` | 0 |
| `podcast:block` `no`, with no `id` | 154 |
| `itunes:block` | 0 |

No feed in the snapshot asks to be blocked. The snapshot holds feeds that were
already in an index, so a blocked feed can be absent from it for that reason.

### The code

No crate reads `podcast:block` or `itunes:block` as a typed field. The parser
keeps channel namespace tags in its generic namespace snapshot.

## 2. How Podcast Index Removes A Feed

Podcast Index has no self-service removal. The maintainers remove, merge and
mark feeds by hand. Discussion 737, from January 2026, says that this work
is too large for the current team:

- A feed can be marked dead, and the index then examines it less often.
- `podcast:block` is named as a way to suspend a show.
- `podcast:txt` with the `verify` purpose proves control of a feed that a
  publisher submitted.
- Contributors proposed a publisher dashboard with removal after proof of
  control, and API functions to delete and merge. Neither exists.

Sources: [discussion 737](https://github.com/Podcastindex-org/podcast-namespace/discussions/737),
and the earlier [Podcast Index evidence](feed-identity-podcastindex-evidence.md).

Thus Podcast Index has no tested self-service solution to copy. The namespace
gives one signal from the source: `podcast:block`.

## 3. A Source-Declared Removal

This design follows ADR 0051: the source URL decides.

- A submission from the source URL with a `podcast:block` that blocks this
  index retires the record. The event is `FeedRetired` with the reason
  `podcast_block`. The node writes no row in `feed_blocks` of ADR 0053.
- A submission with such a tag and no held record is rejected with a new
  reason, for example `source_blocked`. Nothing is written.
- When the publisher removes the tag, the next crawl admits the feed again,
  because no durable block exists.
- A tag in a mirror body changes nothing. ADR 0051 already refuses mirror
  content.
- The tag blocks this index when the value is `yes` and there is no `id`, or
  when the `id` is the slug of this index. A `no` tag with that slug permits
  ingest.

This returns self-service removal after option 6 of the
[ADR 0018 research](adr-0018-proof-flow-history-research.md). It needs no key,
no challenge and no operator. It uses a tag from the namespace. The cost is a
typed parser field, one ingest rule, and an ADR.

Two points need a decision in that ADR:

- The retirement deletes the tracks and their identities. When the feed
  returns, its tracks get the same feed-scoped identities, because ADR 0040
  uses the feed GUID and the item GUID.
- A request for a slug for this index.

## 4. The Wavlake URL Forms

### What Wavlake serves

For one album, Wavlake answers at these URLs:

| Form | Answer on 2026-09-25 | Answer in April |
|---|---|---|
| `https://wavlake.com/feed/music/<id>` | `200` | `200` |
| `https://wavlake.com/feed/<id>` | `200`, the same body | a redirect to the `music` form |
| `https://www.wavlake.com/feed/<id>` | `301` to `https://wavlake.com/feed/<id>` | a redirect to the `music` form |
| `https://wavlake.com/feed/artist/<id>` | a publisher feed, a different feed | the same |

On 2026-09-25 the bare and the `music` forms of one album gave the same body.
The GUID, the five items and the hash without `lastBuildDate` were equal. Each body names the `music` form in
`<atom:link rel="self" href="...">`.

This check used three requests on 2026-09-25. The April column comes from the
`final_url` of the April snapshot.

### What the index holds

The backup of 2026-09-24 20:01 UTC:

| Stored form | Records |
|---|---:|
| `feed/music/<id>` | 6,101 |
| `feed/<id>` | 1,429 |
| `feed/artist/<id>` | 1,619 |

| Stored form, then a URL observed for the same GUID | Pairs |
|---|---:|
| `feed/<id>`, then `feed/music/<id>` | 1,377 |
| `feed/music/<id>`, then `feed/<id>` | 15 |
| `feed/<id>`, then `www` | 13 |
| `feed/music/<id>`, then `www` | 1 |

Most mirror observations are the `music` form of a record stored under the bare
form. A probable cause: a publisher feed lists an album by its `music` form,
and the crawler follows that link. This cause is not verified.

### The self link

The parser already reads `atom:link rel="self"`. The node stores it as a
source link with the path `feed.atom:link[@rel='self']`. In April:

| Host | Self link equal to the requested URL | Equal to the final URL after a redirect | Different | No self link |
|---|---:|---:|---:|---:|
| Wavlake | 5,209 | 1,425 | 9 | 0 |
| All others | 286 | 0 | 1 | 608 |

For Wavlake, the self link names the `music` form almost always.

### The effect of ADR 0051

1,429 records are stored under the bare form. A crawl of that stored URL is an
update, so `refresh` keeps them current. A submission of the `music` form is a
mirror, and answers `source_conflict`. Thus a podping or a publisher link that
names the `music` form does not update these records. The update waits for the
next crawl of the stored URL.

### Options

| Option | Effect | Cost |
|---|---|---|
| A. Do nothing | Correct content. Updates for 1,429 records wait for `refresh` | Slower updates from podping |
| B. A one-time operator relocation | `PATCH` each bare record to its `music` form, after a check that the `music` form gives the same GUID. The records then update from any `music` link | 1,429 signed `FeedUpserted` events. A script. Wavlake can change its forms again |
| C. The self link in the source body is a move trigger | ADR 0052 treats `atom:link rel="self"` at the source URL like `itunes:new-feed-url`. The same checks apply: the target declares the same GUID, no other record holds it, and it passes ADR 0054. Each record moves on its next crawl | An amendment to ADR 0052. The source states the move, so it fits ADR 0051 |
| D. A rule that joins the Wavlake forms | Treats the forms as one URL | A host rule. ADR 0049 deleted the Wavlake host rules, and ADR 0051 adds no URL normalization. Rejected |

### Recommendation

Option C. The source body names its own canonical URL, and the parser
already reads it. The checks of ADR 0052 prevent a wrong self link from moving
a record to a different feed. Option B can run first as a one-time step. Option
C also handles later changes by any host.

## Not Verified

- The form that a podping for a Wavlake album names.
- The 10 records whose self link names a different URL.
- How a publisher client would expect this index to treat `podcast:block` when
  the index has no slug.
