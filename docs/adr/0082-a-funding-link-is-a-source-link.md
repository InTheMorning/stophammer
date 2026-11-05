# ADR 0082: A Funding Link Is A Source Link

## Status
Accepted on 2026-10-31. The operator accepted the proposed values of the five
questions of the proposal. "Open Questions" gives each answer.
Release 0.18.0 holds the change.
[The measurement record](../reviews/adr-0082-refill-measurement.md) gives the
refill pass and the refill check.

## Date
2026-10-31

## Context

The podcast namespace gives a feed a way to name its donation and membership
pages:

```xml
<podcast:funding url="https://www.example.com/donations">Support the show!</podcast:funding>
```

The specification (`docs/tags/funding.md` of `Podcastindex-org/podcast-namespace`)
gives these facts. The parent is `<channel>`, `<item>` or `<podcast:liveItem>`.
More than one tag is permitted in each parent. The attribute `url` is
required. The node value is a free text that an application shows with the
link. The specification asks for 128 characters or fewer, because an aggregator
can cut a longer text.

The index keeps no funding tag. The parser reads `<link>` and the Atom links of
a channel, an item and a live item into `source_links` rows (`extract_links` in
`stophammer-parser/src/engine.rs`). Each row has a `link_type`, a `url`, a
`source`, an `extraction_path` and a position. The types are `website`,
`web_page`, `self_feed` and `content_stream`. The table `source_entity_links`
has no text column, and the node gives each link the source `rss_link`
(`src/api.rs`). ADR 0034 §3 names the table.

The other applications of the operator need the links:

- musicindex-studio writes `podcast:funding` in the channel and in each item
  (Studio ADR 0023, Studio ADR 0075). A feed that Studio writes carries the tag
  today, and the index drops it.
- v4vmm reads `source_links` rows and their `link_type` into its identity link
  facts (`src/views.rs`, `src/metadata_service.rs`).
- musicindex.org reads the feed routes with `include=publisher` and
  `include=link_facts`.

No count of feeds with the tag exists. The node stores no raw namespace tag,
so a backup cannot give the count. The crawler fetch cache holds the gzip body
of each fetched feed, and a search of the bodies gives the count.

Three decisions constrain this one:

- ADR 0077: an ADR that keeps a new value from a feed body states a refill
  pass.
- The archived ADR 0054 §4 and §4a: a query response gives no non-web URL, and
  a `website` or `web_page` value with no scheme reads as `https://` and the
  value.
- ADR 0066 §4: a new response field and a new payload field raise MINOR.

## Decision

### 1. The parser gives each funding tag as a link

The parser reads each `<podcast:funding>` of the channel, of each item and of
each live item, in `extract_links`. Each tag gives one link:

- `link_type` is `funding`.
- `url` is the attribute `url`, with the white space at its ends removed. A tag
  with no `url`, or with an empty one, gives no link.
- `label` is the node value, with the white space at its ends removed. An
  empty node value gives a null label. The parser cuts no text.
- `extraction_path` is `feed.podcast:funding` for the channel, and
  `entity.podcast:funding` for an item and a live item.
- The position follows the sequence of the links of the same parent, as for
  the other link types.

The channel gives a `feed` row, an item gives a `track` row, and a live item
gives a `live_item` row.

### 2. A link has a label

`IngestLink` of the parser and of the node, `SourceEntityLink` of the node and
the table `source_entity_links` get an optional text `label`. A migration adds
the column. The payload of `SourceEntityLinksReplaced` carries the field. A
link of another type has a null label.

A change of the label is a change of the link. The node emits a new
`SourceEntityLinksReplaced` event when only a label changed.

Two funding tags of one parent with the same URL give one row. The first tag
gives the label, as the unique key of the table decides today.

### 3. The source of a funding row is `podcast_funding`

The node gives a funding row the source `podcast_funding`, as an entity ID
from `podcast:txt` has the source `podcast_txt`. Each other link keeps the
source `rss_link`.

### 4. The response gives the label, and no new field elsewhere

Each `source_links` row of the `v1` feed route and track route gives `label`.
The field is null for a link of another type. No route, parameter or include
is added. A client reads the funding links of a feed and of its tracks with
`include=source_links` on the feed route. It keeps the rows with the
`link_type` `funding`.

The rule of ADR 0054 §4 applies to a funding `url`: a value with a scheme other
than `http` or `https` reads as null. The rule of §4a does not apply. A funding
value with no scheme reads as null, and `url_scheme_added` is false. The
specification requires a URL, and the index adds no scheme to a value that is
not one.

### 5. Out of scope

- **Search.** No filter and no index text comes from a funding link.
- **Display rules.** How an application shows a funding link is a rule of that
  application, or a shared rule of musicindex. This ADR gives the data only.
- **Live item rows.** The funding rows of a live item are stored. The live item
  routes of ADR 0064 give no `source_links`, and this ADR changes them not.
- **The value block.** `podcast:value` stays a payment route (ADR 0034). A
  funding link is not a payment route.

## Open Questions

None. The operator answered the five questions of the proposal on
2026-10-31:

1. The name of the text field is `label` (§2).
2. The index keeps the label as given, and an application cuts it for display
   (§1).
3. ADR 0054 §4a does not apply to a funding row. A funding value with no
   scheme reads as null (§4).
4. The refill pass is a forced `refresh` of each held feed, because the fetch
   cache holds not each feed ("Consequences").
5. The funding rows of a live item are stored, and no live route gives them
   (§5).

## Alternatives Considered

### A table of funding links

A table `feed_funding_links`, an event `FeedFundingReplaced` and a field
`fundings` on the feed and track responses. This gives a client a direct field.
It also gives a second path for a link from RSS, in addition to
`source_links`, with its own apply, sync and query code. v4vmm reads `source_links` rows by
`link_type` today, so the existing path gives the client the data with one new
filter value.

### A funding tag as a `podcast:txt` entity ID

The table `source_entity_ids` has a `scheme` and a `value`. A funding link is a
URL with a display text, not an identifier, and the table has no text column
either.

### A new field with no label

The `url` alone misses the text that the creator wrote for the link. Studio
edits that text, and an application shows it.

## Consequences

- **The refill pass (ADR 0077 §1).** After the deploy, the operator counts the
  bodies of the crawler fetch cache that hold the text `podcast:funding`. Then
  the operator runs `stophammer-crawler --force --no-revalidate refresh` over
  each held feed. `--no-revalidate` makes the crawler fetch each body, and
  `--force` makes the node read a body whose hash it holds.
- **The refill check (ADR 0077 §1).** The check has two parts. A backup after
  the pass gives
  `SELECT count(DISTINCT feed_guid) FROM source_entity_links WHERE link_type = 'funding'`,
  and the count is compared with the cache count. One known feed with the tag
  gives its rows in `GET /v1/feeds/{guid}?include=source_links`. `AGENTS.md`
  names the pass in "Open Items" until the check passes (ADR 0077 §2).
- **The version.** A new response field and a new payload field raise MINOR
  (ADR 0066 §4). The release is 0.18.0. The three repositories get the version
  together (ADR 0066 §3). The crawler sends the parser types, so it changes
  only its parser dependency and its version.
- **Community nodes.** A node of release 0.17.0 applies a
  `SourceEntityLinksReplaced` payload with the new field and drops the label,
  because the payload types ignore an unknown field. The release notes tell a
  community operator to upgrade before the refill pass, so that the node keeps
  the labels.
- **The change spans two repositories.** This repository holds the ADR. The
  `stophammer-parser` commit names it.
- **The guards of the contract.** `tests/fixtures/v1-schema-fields.json` gets
  `label` in `SourceEntityLinkResponse`, and the migration gets its line in
  `tests/fixtures/migration_sha256.txt`.
- Tasks 001 to 003 in `docs/tasks/` give the build. Task 001 changes the
  parser, and tasks 002 and 003 change the node.

## Verification

Mechanical. Each criterion is a test:

- In `stophammer-parser`: a fixture with two channel tags, one item tag and one
  live item tag gives four links of the type `funding`. Each link has its label
  and its extraction path, in sequence after the other links of the same
  parent. A tag with no `url` gives no link. A tag with an empty node value
  gives a null label. A label of more than 128 characters is kept whole.
- In `stophammer`, the storage: the migration has its hash line.
  `SourceEntityLink` and the payload round-trip the label. A link set that
  differs only in a label gives a new event.
- In `stophammer`, the response: the feed route with `include=source_links`
  gives `label` on each row. A `funding` row with a `javascript:` URL gives a
  null `url`. A `funding` row with no scheme gives a null `url` and
  `url_scheme_added` false. The schema fixture holds `label`.

Manual, because a test cannot see the production corpus:

- The refill check of "Consequences".
