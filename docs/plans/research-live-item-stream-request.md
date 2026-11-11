# Research Request: A Live Item Keeps Its Content Link Apart From Its Stream

## Status

Answered - 2026-11-09. One check is open.

| Section | Status | Owner or blocker |
|---|---|---|
| L1, read each `podcast:contentLink` child | Answered | [ADR 0093](../adr/0093-a-live-item-gives-its-content-link-not-its-stream.md) §1 and §2 |
| L2, keep the stream apart from the content link | Answered | ADR 0093 §1 and §3 |
| L3, the 17 stored rows | Answered | ADR 0093 §4. Migration 0064 cleared them |
| The read of `GET /v1/live-items` after one day | Open | It occurs on 2026-11-12. A feed that keeps a null `content_link` while its body has `podcast:contentLink` gets the replay of ADR 0093 §4 |

Received - 2026-11-09. This request comes from the research repository at
`~/build/musicindex-research`, commit `82f56fe`. That repository has no Git
remote. Its index is `AGENTS.md`. The note is
`research_notes/podcast-apps/live-item-stream-and-content-link.md` in that
repository.

This document binds nothing in Stophammer. It asks Stophammer to examine one
change and to make a decision. Stophammer records the decision in its own ADR.

The data is in `sources/captures/2026-11-09-live-item-stream/` in the research
repository. `SHA256SUMS` in that folder gives the hashes. The research examined
the parser at commit `7130556` and the backup
`backups/stophammer-20261109T010844Z.db`.

Stophammer examined this request on 2026-11-09 and agrees.
[ADR 0093](../adr/0093-a-live-item-gives-its-content-link-not-its-stream.md) is
Accepted. It answers L1, L2 and L3. On 2026-11-11 the primary runs it in the
candidate `v0.19.0-rc.8`, and no release holds it.

## Purpose

| # | Change | Evidence |
|---|---|---|
| L1 | Read each `<podcast:contentLink>` child element of a live item, with its `href` and its text | The namespace and each examined feed give a child element. No examined feed gives the attribute that the parser reads |
| L2 | Keep the stream and the content links in different fields. Do not put the enclosure URL in `content_link` | Each of the 17 `live_events` rows of the backup has a stream URL as `content_link` |
| L3 | Decide what occurs to the 17 stored rows | Three live items have a content link page that the index does not keep: Boost Beach and two Polar Embrace items |

## The Defect

At parser commit `7130556`, `src/engine.rs:323-326` reads `contentLink` as an
attribute of the live item. When the attribute is not there, the code uses the
enclosure URL. `src/engine.rs:1125-1135` adds a `content_stream` link from the
same attribute, with the path `live_item.@contentLink`.

The namespace gives the element as a child, with this shape:

```xml
<podcast:contentLink href="https://zine.bitpunk.fm/live">Listen Live!</podcast:contentLink>
```

Thus each stored `content_link` is the stream of its live item.

## Stophammer Check

- In the backup, each of the 17 `content_link` values is equal to a stored
  enclosure URL of the same live item in `source_item_enclosures`.
- No `source_entity_links` row of the backup has the type `content_stream`.
- The ingest keeps the enclosure of a live item in `source_item_enclosures`.
  Thus the stream has its own field.
- v4vmm shows `content_link` as a link out of its live page.

## The Answer

ADR 0093 gives each answer:

- L1: §1 and §2. The first `href` gives `content_link`. Each element gives a
  `content_link` link row, with its text as the label.
- L2: §1 and §3. The parser does not use the enclosure in `content_link`. The
  live item reads add no stream field.
- L3: §4. Migration 0064 clears each stored value that is an enclosure of its
  live item. The correct content link comes with the next changed body, or with
  a replay of the fetch cache.

The parser test `stophammer-parser/tests/live_content_link.rs` uses the
captured feed `bitpunk-live.xml` as `tests/fixtures/bitpunk_live.xml`.
