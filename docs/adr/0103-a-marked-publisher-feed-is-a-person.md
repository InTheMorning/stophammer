# ADR 0103: A Marked Publisher Feed Is A Person

## Status
Proposed on 2026-11-16. No release holds it.

- Bar: crosses repositories. The parser, the node verifier, the read view,
  musicindex-studio and the Publisher Feed Generator plugin change together.
- Changes: [ADR 0101](0101-a-publisher-feed-can-list-playlists.md) §1
- Uses: ADR 0044 §4 for a `v1` addition.

## Date
2026-11-16

## Context

The operator wants each artist who makes a publisher feed with the phafe tool
to show as a person in musicindex-studio. The phafe tool is the Publisher Feed
Generator, a WordPress plugin on `https://phafe.com/roles-feed-generator`. A
copy is in
`musicindex-research/sources/music-feeds/editors/publisher-feed-generator`.

On 2026-11-16 the main session read the plugin copy and the code of the suite:

- The plugin makes only feeds with `podcast:medium` `publisher`. The list of
  releases can be empty: `class-pfg-feed.php` sets a maximum and no minimum.
- The plugin sends no podping (`docs/DECISIONS.md`, "not built").
- `MediumMusicVerifier` (`src/verifiers/medium_music.rs`) rejects a publisher
  feed with no `music` or `musicL` item (ADR 0101 §1). So the node rejects the
  feed of a new artist with no release.
- The parser reads `podcast:txt` only for `purpose="npub"`
  (`extract_entity_ids` in `stophammer-parser/src/engine.rs`). The node stores
  the value in `source_entity_ids` with the scheme `nostr_npub`, and
  `include=source_ids` gives it. The signed ingest event carries it.
- The Studio people lookup reads `GET /v1/search?type=publisher` with
  `include=link_facts` (`musicindex-studio/crates/core/src/musicindex.rs`). It
  puts a feed with no confirmed link in the level `NameOnly`, with the label
  "Unverified" (`crates/core/src/lookup.rs`).
- The production copy `stophammer-20261115T115439Z.db` holds 1,811 publisher
  feeds. 29 of them have no album that names them with `<podcast:publisher>`.
  The count matches by GUID only, so it is an approximation.

The operator made these choices on 2026-11-16:

- The index must not look for podcast spam that is not related to music. The
  index cannot stop a person who publishes a bad feed on purpose.
- A mark in the RSS is sufficient. The mark is a `podcast:txt` element.

## Decision

### 1. The mark

The person mark is a channel element of the Podcast Namespace:

```xml
<podcast:txt purpose="musicindex">person</podcast:txt>
```

The parser compares `purpose` and the text with ASCII case ignored, after it
removes the white space at each end. An item-level element is not a mark.

### 2. The parser keeps the mark

`extract_entity_ids` maps `purpose="musicindex"` to the scheme `musicindex`.
The node stores each such value in `source_entity_ids`, as it stores
`nostr_npub`. `include=source_ids` gives it. The signed ingest event carries
it, so a community node stores the same row.

### 3. The node accepts a marked publisher feed with no release

This section changes ADR 0101 §1.

`MediumMusicVerifier` accepts a publisher feed when one of these conditions is
true:

- The feed has a minimum of one channel `podcast:remoteItem` with the medium
  `music` or `musicL` (ADR 0101 §1).
- The feed has the mark of §1.

A publisher feed with neither stays rejected. The reason names the two
mediums and the mark. Each other verifier does not change.

### 4. The read gives the mark

A publisher feed has the mark when its stored `source_entity_ids` hold a feed
row with the scheme `musicindex` and the value `person`. The field
`person_mark` is a boolean. It is on these rows, and on a feed of each other
medium it is absent:

- `GET /v1/feeds/{guid}` of a publisher feed, with no include.
- Each `publisher` row of `GET /v1/search`, with no include.
- Each `publisher` row of `GET /v1/feeds/recent`, `GET /v1/feeds/by-name` and
  `GET /v1/artist-names` with `include=link_facts`.

The mark does not change a link fact or a count. A marked feed with no
confirmed link keeps `two_way_link_count` 0.

### 5. The crawler does not change

- The crawler finds a marked feed through a podping, through the PodcastIndex
  snapshot that `import` reads, or through a release that names it. Each of
  these paths exists. `import` reads the medium `publisher`
  (`is_index_medium` in `stophammer-crawler/src/feed_skip.rs`).
- The crawler does not look for publisher feeds in a new source.
- The skip store does not skip a publisher URL after a `[medium_music]`
  rejection (ADR 0101 §4). So a feed that adds the mark later enters at its
  next fetch.

### 6. One capability names the addition

`GET /v1/node/capabilities` names `person_mark` in `capabilities`.

### 7. Work in other repositories

These items are not decisions of Stophammer. The orchestrator sends them to the
owners:

- The Publisher Feed Generator writes the mark of §1 in each feed that it
  makes.
- The Publisher Feed Generator sends a podping after each save. Without it, the
  index finds a new feed only through the snapshot of PodcastIndex or a
  release.
- musicindex-studio shows a row with `person_mark` true as a person, also with
  no confirmed link. Studio owns the level and the label.

## Alternatives Considered

### Accept each publisher feed with no release

The gossip crawler fetches each podping URL. Each empty publisher feed would
then enter the index, also a feed that is not related to music. The operator
rejected this on 2026-11-16.

### A list of hosts that the operator sets

The node would accept an empty publisher feed only from a host such as
`phafe.com`. RSS text cannot fake that mark. Each new host needs an operator
change, and a community node needs the same setting. The operator chose on
2026-11-16 that a mark in the RSS is sufficient. Rejected.

### The text of `<generator>`

Each feed of the plugin has `<generator>Phafe RSS Feed</generator>`, so the
plugin needs no change. The parser does not read `<generator>`. The mark then
names one tool, and each new tool needs a node change. Rejected by the operator
on 2026-11-16.

### A proof by Nostr key

The node would check the key of the feed against the website of the feed. The
node has no Nostr code, and the check needs a fetch at each ingest. Rejected.

## Consequences

- An artist with no release can be in the index, and Studio can show the artist
  as a person.
- A person can write the mark in a feed that is not an artist feed. The
  operator accepts this. The feed must have the medium `publisher` and the mark,
  so spam that is not related to music does not enter.
- The 29 publisher feeds with no named album stay as they are. They have no
  mark, so Studio does not show them as people.
- The parser, the node and Studio change. The parser change is in the
  `stophammer-parser` repository and names this ADR.
- A marked feed with no release gives no link fact. Its releases add link facts
  when they name it.

## Acceptance

Each criterion is mechanical.

1. A parser unit test: a channel `<podcast:txt purpose="musicindex">person</podcast:txt>`
   gives an entity ID with the scheme `musicindex` and the value `person`. The
   same element with `purpose="MusicIndex"` and the text ` Person ` gives the
   same result. An item-level element gives no channel entity ID.
2. A unit test of `MediumMusicVerifier`: a publisher feed with the mark and no
   remote item passes. A publisher feed with no mark and no remote item fails,
   and the reason names the mark.
3. A router test: an ingest of a marked publisher feed with no remote item
   gives `200`. `GET /v1/feeds/{guid}` gives `person_mark` true, and
   `include=source_ids` gives the `musicindex` row.
4. A router test: `GET /v1/search?type=publisher` gives `person_mark` true on
   the marked feed and false on an unmarked publisher feed.
5. A router test: `GET /v1/feeds/recent?medium=publisher&include=link_facts`
   gives `person_mark` on each publisher row. A music row does not give it.
6. A unit test: `GET /v1/node/capabilities` names `person_mark`.
7. The OpenAPI document and `tests/fixtures/v1-schema-fields.json` name
   `person_mark`.
