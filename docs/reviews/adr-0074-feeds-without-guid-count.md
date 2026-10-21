# ADR 0074: The Count Of The Records Without A Source GUID

Date: 2026-10-21. This record gives the measurement of
[ADR 0074 §4](../adr/0074-a-feed-without-a-guid-is-invalid.md).

## Method

The fetch cache of the crawler (ADR 0050) holds the last body of each fetched
URL. A copy of that cache was compared with a copy of the node database of
2026-10-19. A body is in the count when its channel has no non-empty
`podcast:guid` element that is not in an `<item>`. A record is in the count when
the body at its stored `feed_url` is in the count.

A channel element can follow the items. The first version of Music Side
Project Studio writes `<title>`, `<link>` and `<podcast:guid>` after the last
item. A first count looked only before the first `<item>`, and it gave 35
records in error. The parser reads such a channel GUID correctly.

## Result

| Measure | Count |
|---|---|
| Cached bodies | 12350 |
| Cached bodies with no `podcast:guid` | 239 |
| Records in the node database | 10581 |
| Records with no cached body at their `feed_url` | 226 |
| Records with a cached body with no `podcast:guid` | 2 |

The cached body of each of the two records is not RSS:

- `feeds.feedburner.com/musichour` gives an HTML page.
- `willshare.com/podcasts/things/things.xml` gives binary data that was
  decoded as text. The bytes cannot be read again.

Thus the cache shows no record with an RSS body and no `podcast:guid`. It
cannot show if the two feeds give a GUID.

## Limits

- The 226 records with no cached body are not in the count.
- The other 237 cached bodies with no `podcast:guid` are not at the
  `feed_url` of a record. §4 does not apply to them.
