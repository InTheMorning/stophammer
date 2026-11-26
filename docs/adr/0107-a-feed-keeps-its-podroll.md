# ADR 0107: A Feed Keeps Its Podroll

## Status
Accepted on 2026-11-26. On 2026-11-26 the operator asked, through the
orchestrator session, that the suite carries the podroll of each feed. The
orchestrator session gave the selection of the shape to this session. This
session selected the shape below. No release holds it.
[Task 001](../tasks/adr-0107-task-001-parser-reads-the-podroll.md) and
[task 002](../tasks/adr-0107-task-002-node-keeps-the-podroll.md) give the
work.

- Bar: crosses repositories, and an agent can make the opposite choice by
  mistake. The `stophammer-parser` change and the node change go together.
  Without this record, an agent can put a podroll entry in
  `feed_remote_items_raw`, where each reader takes it as a link of the feed.
- Uses: [ADR 0059](0059-an-entry-that-names-a-feed-gives-its-summary.md) §1 for the summary of
  a named feed, [ADR 0066](0066-a-version-number-tells-what-to-upgrade.md)
  for the version, [ADR 0077](0077-a-new-body-value-states-its-refill-pass.md)
  for the refill pass, and [ADR 0106](0106-a-feed-changes-its-guid-in-place.md)
  §2 for a GUID change.

## Date
2026-11-26

## Context

The podcast namespace gives a channel a list of other feeds that the creator
recommends:

```xml
<podcast:podroll>
  <podcast:remoteItem feedGuid="917393e3-1b1e-5cef-ace4-edaa54e1f810" feedUrl="https://example.com/feed.xml" medium="music"/>
</podcast:podroll>
```

The specification (`docs/tags/podroll.md` of
`Podcastindex-org/podcast-namespace`) gives these facts. The parent is
`<channel>`. The tag holds one or more `<podcast:remoteItem>` elements. Each
remote item must have `feedGuid`, and can have `feedUrl` and `medium`.

The research note
`musicindex-research/research_notes/music-feeds/funding-and-podroll.md`
(commit `002b6d0`), sections "Podroll" and "Gaps" items 1 and 6, gives the
state of the suite:

- `extract_feed_remote_items` in `stophammer-parser/src/engine.rs` reads the
  direct remote item children of the channel and of `podcast:publisher`. It
  skips the children of `podcast:podroll`. Only the generic
  `podcast_namespace` snapshot keeps them, and the node does not read that
  snapshot.
- musicindex-studio writes the tag. The index drops it, so no client that
  reads the index can show a podroll.
- No test covers a podroll value in the parser.

The table `feed_remote_items_raw` holds the channel remote items. Its readers
take each row as a link of the feed:

- The publisher view takes each `medium="music"` row of a publisher feed as
  an album of the publisher (`src/query.rs`, `build_publisher_row`).
- The playlist list takes a publisher feed with a `medium="musicL"` row as a
  publisher of playlists (`src/query.rs`, ADR 0101).
- The publisher link statistics count each `medium="music"` row of a
  publisher feed (`get_publisher_link_stats` in `src/db.rs`).
- The response `remote_items` gives each row. A client reads the rows of a
  `musicL` feed as the entries of the playlist (ADR 0060).

These readers select by `medium`, not by `source`.

## Decision

### 1. The parser gives the podroll as its own field

`IngestFeedData` and the feed values of the parser get the field
`podroll: Vec<IngestRemoteFeedRef>`. The field reuses the type of the channel
remote items. It is not a part of `remote_items`.

- The parser reads each `podcast:remoteItem` child of each channel
  `podcast:podroll`, in the sequence of the body.
- An entry has the same values as a channel remote item: `feedGuid` trimmed,
  `feedUrl`, `medium` as written, and the other attributes.
  The parser sets `publisher_reference` to `false`. An item with no `feedGuid`, or with an
  empty `feedGuid`, gives no entry.
- The podroll has its own positions, from 0. They are not related to the
  positions of `remote_items`.
- `remote_items` does not change. Thus the verifiers and each reader of
  `remote_items` see no podroll entry.
- The field has `serde(default)`, so a body that a crawler of an earlier
  release sends gives an empty podroll.

### 2. The node keeps the podroll in its own table

A migration adds the table `feed_podroll_items`:

| Column | Type |
|---|---|
| `id` | `INTEGER PRIMARY KEY` |
| `feed_guid` | `TEXT NOT NULL REFERENCES feeds(feed_guid)` |
| `position` | `INTEGER NOT NULL` |
| `medium` | `TEXT` |
| `remote_feed_guid` | `TEXT NOT NULL` |
| `remote_feed_url` | `TEXT` |

The table is `STRICT`, with `UNIQUE(feed_guid, position)` and an index on
`remote_feed_guid`. The node keeps only these four values of an entry. Each
ingest replaces the rows of the feed, as for `feed_remote_items_raw`.

- The table is in `GUID_CHANGE_COLUMNS` for `feed_guid`, and in
  `GUID_CHANGE_STAYS` for `remote_feed_guid` with the reason "a different
  feed" (ADR 0106 §2).
- Each path that deletes the rows of a feed from `feed_remote_items_raw` also
  deletes its rows from `feed_podroll_items`.
- No reader of `feed_remote_items_raw` reads the new table. The feed copy
  comparison does not compare the podroll.

### 3. The existing event carries the podroll

`FeedRemoteItemsReplacedPayload` gets the field
`podroll: Option<Vec<FeedPodrollItem>>`, with `serde(default)` and no output
when it is `None`.

- The primary always sends `Some`, with the full podroll of the feed.
- An ingest gives the event when the remote items or the podroll changed.
- A node that applies the event with `Some` replaces the podroll rows of the
  feed. With `None`, the node does not change them. Thus an event that a node
  signed before this ADR keeps its meaning.
- A node of an earlier release ignores the field, and keeps working with no
  podroll.

### 4. The feed route gives the podroll with `include=podroll`

`GET /v1/feeds/{guid}` gets the include value `podroll`. The response gets
the field `podroll`: a list in the sequence of `position`, or no field when
the request does not ask for it. Each entry gives:

| Field | Value |
|---|---|
| `position` | The stored position |
| `medium` | The stored medium, as written in the body |
| `remote_feed_guid` | The stored GUID |
| `remote_feed_url` | The stored URL, or null |
| `remote_feed_title` | The title of the named feed, or null when the node does not hold it (ADR 0059 §1) |
| `remote_feed_image_url` | The image of the named feed, or null, as ADR 0059 §3 gives it |
| `remote_release_artist` | The release artist of the named feed, or null (ADR 0059 §1) |

The node finds the named feed with the same resolution as a remote item
(`resolve_listed_feed`). The list of include values in the OpenAPI document
and in `docs/API.md` gets `podroll`. `include=remote_items` does not change.

### 5. Out of scope

- The track routes. The specification permits `podcast:podroll` only in the
  channel.
- The live chain, HitIt, Studio and the website. Each one reads the API
  field when its owner decides.

## Alternatives Considered

### A podroll entry is a remote item with its own source

`feed_remote_items_raw` has a `source` column, which ADR 0069 uses for a
publisher reference. A row with `source = 'podcast_podroll'` needs no new
table and no new payload field. But each reader that selects by `medium`
must also exclude the new source, and each subsequent reader must remember to
do so. A podroll entry with `medium="music"` in a publisher feed would become
an album of the publisher, and an entry in a `musicL` feed would become an
entry of the playlist. A community node of an earlier release would apply
such rows and show them so, with no error. Rejected.

### A new event type

An event `FeedPodrollReplaced` keeps the payloads apart. A node of an earlier
release cannot read a new event type, so each community node must upgrade
before the primary sends it (ADR 0066). A new field of the existing payload
lets an old node keep working. Rejected.

### The generic snapshot of the namespace

The parser already keeps the podroll in its `podcast_namespace` snapshot. The
node does not keep the snapshot, and a typed field gives a test a value to
examine. Rejected.

## Consequences

- **The refill pass (ADR 0077 §1).** After the deploy, the operator counts
  the bodies of the crawler fetch cache that hold the text `podcast:podroll`.
  Then the operator runs `stophammer-crawler --force --no-revalidate refresh`.
  The `refresh` mode reads each feed that the node holds. `--no-revalidate`
  makes the crawler fetch each body, and `--force` makes the node read a body
  whose hash it holds.
- **The refill check (ADR 0077 §1).** The check has two parts. A backup after
  the pass gives
  `SELECT count(DISTINCT feed_guid) FROM feed_podroll_items`, and the count
  is compared with the cache count of the held feeds. One known feed with the
  tag gives its entries in `GET /v1/feeds/{guid}?include=podroll`. `AGENTS.md`
  names the pass in "Open Checks" until the check passes (ADR 0077 §2).
- **The version.** A new response field, a new include value and a new
  payload field raise MINOR (ADR 0066 §4). The release is 0.23.0. The three
  repositories get the version together. The crawler sends the parser types,
  so it changes only its parser dependency, its test feeds and its version.
- **Community nodes.** A node of release 0.22 applies the event and drops the
  podroll. The release notes tell a community operator to upgrade before the
  refill pass, so that the node keeps the podroll.
- **The guards of the contract.** `tests/fixtures/v1-schema-fields.json` gets
  `podroll` in `FeedResponse` and the new entry schema. The migration gets its
  line in `tests/fixtures/migration_sha256.txt`.
- **HitIt and other users of the parser** that build `IngestFeedData` must
  add the field when they move to the new parser revision.

## Verification

Mechanical. Each criterion is a test:

- In `stophammer-parser`: a fixture with a channel remote item and a podroll
  of three items gives one entry in `remote_items` and the podroll entries
  with their GUID, URL, medium and positions 0 and 1. A podroll item with no
  `feedGuid` gives no entry. An item `podcast:podroll` gives no channel
  entry.
- In `stophammer`: an ingest of a feed with a podroll stores its rows, and
  `include=podroll` gives them with the summary of a held named feed. The
  publisher view and `include=remote_items` do not give a podroll entry.
- In `stophammer`: a body with a changed podroll and the same remote items
  gives a `FeedRemoteItemsReplaced` event with the podroll. A node that
  applies it has the same rows. An event with no `podroll` field leaves the
  rows of the node.
- In `stophammer`: a GUID change moves the podroll rows, and a retire deletes
  them. The ADR 0106 guard of the migrated schema covers the table.
