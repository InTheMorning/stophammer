# ADR 0083: The Self Links Of Moved Feeds

Date: 2026-11-04. This record gives the measurement that
[ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md) used. It states no rule.

## Method

The research repository measured 35 feed pairs on 2026-11-04. The capture is
`sources/captures/2026-11-04-self-links/` in `musicindex-research`, at its commit
`1d35f24`. The capture hashes pass. In each pair, the tag of one feed, the origin,
names the other feed, and the tag of the other feed, the destination, names itself.
The research fetched each of the 70 URLs one time, and each answered `200`.

## Result

| The self link | Destinations | Origins |
|---|---|---|
| Names its own URL | 25 | 4 |
| Names its own URL, with a different spelling | 1 | 1 |
| Names the other URL | 1 | 22 |
| Names a third URL | 0 | 0 |
| The body has no self link | 8 | 8 |
| The fetch failed, or the body has a different GUID | 0 | 0 |

The origin count of 22 holds one self link with a different spelling.

## What This Shows

The sample selected destinations whose tag names itself. So it gives no evidence
about a body with no tag, which was the question that the measurement was to answer.
The operator accepted ADR 0083 with this limit on 2026-11-04.

One destination has the form of scenario S15: its tag names itself, and its self link
names the origin. The two URLs serve the same bytes. ADR 0083 moves that record, and the
chain rule of the research request does not.

35 pairs from a sample give no rate for all feeds.
