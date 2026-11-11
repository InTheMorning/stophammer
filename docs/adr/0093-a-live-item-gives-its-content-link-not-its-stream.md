# ADR 0093: A Live Item Gives Its Content Link, Not Its Stream

## Status
Accepted on 2026-11-09, for the research request of 2026-11-09. Release
0.19.0 holds it.

It changes the source of `content_link` in ADR 0064 §4 and §6. It adds the link
type `content_link` to the label rule of ADR 0082 §2.

## Date
2026-11-09

## Context
[The research request of 2026-11-09](../plans/research-live-item-stream-request.md)
reports a parser defect. At parser commit `7130556`, the parser reads
`contentLink` as an attribute of `podcast:liveItem`. When the attribute is not
there, it uses the enclosure URL as `content_link`.

The Podcast namespace gives the content link as a child element:

```xml
<podcast:contentLink href="https://zine.bitpunk.fm/live">Listen Live!</podcast:contentLink>
```

No examined feed gives the attribute. Thus each stored `content_link` is the
stream of the live item, not a page for a person.

In the backup `stophammer-20261109T010844Z.db`, `live_events` has 17 rows in
6 feeds. Each `content_link` value is equal to a stored enclosure URL of the
same live item in `source_item_enclosures`. No stored link row has the type
`content_stream`. Three live items of these feeds have a content link page that
the index does not keep.

The node keeps the stream apart. The ingest writes the enclosure and
each `podcast:alternateEnclosure` of a live item to `source_item_enclosures`.

v4vmm shows `content_link` as a link out of its live page. A stream URL in that
field sends a person to a raw media file.

## Decision

### 1. The parser reads each content link child element

The parser reads each `<podcast:contentLink>` child element of a live item in
the Podcast namespace.

- `content_link` is the `href` of the first such element with a non-empty
  `href`, with the white space at its ends removed.
- `content_link` is null when the live item has no such element.
- The parser does not use the enclosure URL, or a different value, as the
  content link.

### 2. Each content link is a link row with a label

Each such element with a non-empty `href` gives one link of the live item:

- `link_type` is `content_link`.
- `url` is the `href`, with the white space at its ends removed.
- `label` is the node value, with the white space at its ends removed. An
  empty node value gives a null label.
- `extraction_path` is `live_item.podcast:contentLink`.

The link type `content_stream` and the path `live_item.@contentLink` are
removed. No stored row has them.

ADR 0082 §2 gives a label only to a `funding` link. A `content_link` link also
has a label. The read of ADR 0082 §4 applies to it: a value with a scheme other
than `http` or `https` reads as null.

### 3. The stream stays in its own field

The enclosure of a live item stays in `source_item_enclosures`, as before. It
does not go into `content_link`.

The live item reads of ADR 0064 §6 give no stream field. This ADR adds none.
A client request decides such a field.

### 4. A migration clears each stored stream

Migration 0064 sets `content_link` to null in a `live_events` row when the
value is equal to an enclosure URL of the same live item. The enclosure URLs
are in `source_item_enclosures`. In the backup of 2026-11-09, it clears 17 of 17 rows.
The migration emits no event. A community node runs the same migration.

A crawl of a feed with the same body writes nothing. Thus a row gets its correct
content link when the body of its feed changes, or when the operator replays
the fetch cache with `export-feed-cache-ndjson.py`.

## Consequences
- `content_link` is a page for a person, or null. A client that read the
  stream from `content_link` gets null.
- A live item with no content link keeps its row. ADR 0064 §4 refuses no live
  item for a missing content link.
- A crawler image with the parser change sends the correct content links. A
  node with migration 0064 and an older crawler stores the enclosure again, so
  the crawler image must go with the node release.
- The mechanical checks are the parser tests
  `stophammer-parser/tests/live_content_link.rs` and the node test
  `tests/adr0093_live_content_link_tests.rs`. No visual criterion applies.
