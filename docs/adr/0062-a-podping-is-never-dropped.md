# ADR 0062: A Podping Is Never Dropped

## Status
Accepted on 2026-09-26

## Date
2026-09-26

## Context
The `gossip` mode of `stophammer-crawler` reads podping notifications. Before
it crawls a URL, it asks `Dedup::should_process` in `src/dedup.rs`:

- The first podping for a URL is crawled.
- Each later podping for that URL within 5 minutes is dropped. It is not
  delayed.
- After 5 pings in the window, the window becomes 30 minutes.

No ADR or plan gives a reason for these values. They came with the rewrite of
the crawler in commit `c02bbae` of 2026-03-12. The follow URLs of ADR 0049
§2 use the same state, so a followed feed also closes the window of its URL.

The podping archive of the operator gave these counts for the 7 days before
2026-09-26:

| Measure | Value |
|---|---|
| Podping messages | 384,814 |
| Different URLs | 101,966 |
| Messages with the reason `live` or `liveEnd` | 82 |
| Messages with `medium` other than `podcast` | 5 |
| A repeat of a URL within 15 seconds | 39,163 |
| A repeat within 15 to 30 seconds | 30,658 |
| Podpings that the 5-minute cooldown drops | 204,278, 53% |
| Podpings that a 30-second cooldown drops | 61,181, 16% |
| The most pinged URL | 11,915 pings, one each 51 seconds |
| Feeds of this index with a podping | 10, with 13 podpings |
| Podpings of this index that the cooldown drops | 1 |

Thus:

- A sender almost never gives the medium. The crawler learns the medium only
  from the feed.
- A podping for a feed of this index is rare. Each one can be the only signal
  of a change, and Wavlake sends none.
- The high volume comes from podcasts. The skip list of the crawler stops a
  known non-music URL before a fetch, so a podcast podping costs a database
  read.
- A dropped podping loses a change until the next podping, which can be days
  later. A podping can also arrive before the CDN of the host serves the new
  file. Then the first crawl reads the old file, and the podping that follows
  is dropped.

ADR 0050 makes a fetch of an unchanged feed a conditional GET. It costs a `304`
with no body.

## Decision

### 1. A podping in the window is merged, not dropped

Each URL has a window. The first podping of a URL with no open window is
crawled at once, and a window opens. Each podping inside the window sets one
pending mark on the URL. When the window closes and the mark is set, the
crawler crawls the URL one more time, and a new window opens.

So a burst of podpings costs at most two crawls. A crawl always follows the
last podping of a burst.

### 2. The base window is 30 seconds

A new URL, and a URL whose last crawl changed the index, has a window of 30
seconds.

### 3. The window grows for a URL that does not change

After each crawl of a URL, the crawler sets the next window from the result:

| Result | Next window |
|---|---|
| The node accepts a change | 30 seconds |
| A `304`, or the node answers `no_change` | Two times the last window, at most 1 hour |
| A fetch error, or a `429` | Two times the last window, at most 1 hour |
| The skip list stops the URL | No crawl. The window does not change |

The crawler reads the answer of the node, not only the HTTP status. A feed that
changes only its `lastBuildDate` at each podping gets a `200` and `no_change`,
so its window grows.

### 4. A live notification is not delayed

A podping with the reason `live` or `liveEnd` is crawled at once, also inside a
window. It does not change the window. A crawl of the URL that runs at that
time takes the place of a new one.

### 5. A follow URL has its own cooldown

The crawler makes a follow URL of ADR 0049 §2 itself. It is not a signal of a
publisher, and many albums can name one publisher. So a follow URL keeps a
cooldown of 5 minutes, and a follow URL inside it is not fetched again. This
cooldown is apart from the podping window. A follow fetch does not open or
change a window, so a follow never delays or drops a podping.

### 6. The state is in memory

The windows and the pending marks are in memory. A restart clears them. The
archive replay of ADR 0031 after a restart then crawls each URL at most one
time for each window.

### 7. What does not change

These parts do not change:

- the skip list,
- the per-host throttle of the crawler,
- the follow limits of ADR 0054 §3,
- the medium check of the node. `newValueBlock`
notifications stay filtered.

## Alternatives Considered

### A 15-second or a 30-second cooldown that drops
A shorter cooldown drops fewer podpings: 16% at 30 seconds. It still drops, so
a change can still be lost. Rejected.

### Keep the 5-minute cooldown
It drops 53% of podpings, and it lost 1 of the 13 podpings of this index in 7
days. Rejected.

### No window
Each podping gives a crawl. The URL with a podping each 51 seconds gives 1,700
crawls in a day. The skip list stops it only when it is a known non-music feed.
Rejected.

### Grow the window on the HTTP status only
A feed that rewrites `lastBuildDate` at each podping gives a `200` each time,
and its window never grows. Rejected.

## Consequences

- No podping for a feed of this index is lost. A second change is read at
  most 30 seconds after the first crawl, when the feed changed.
- A URL that pings with no change reaches a window of 1 hour after 7 crawls
  with no change. The crawl at the end of a window opens the next window, so
  such a URL then costs about 24 crawls in a day. Most of them are `304`
  answers.
- A publisher whose CDN is slow gets the new file on the crawl at the end of
  the window.
- The crawler holds one small entry in memory for each URL with an open
  window.

## Invariants

- A podping is never dropped. It gives a crawl, or it sets the pending mark of
  a crawl.
- A URL has at most one crawl in progress.
- Only an accepted change resets the window to 30 seconds.

## Guards

- Three podpings for one URL within 10 seconds give one crawl at once and one
  crawl when the window closes.
- A podping inside the window of a URL, with no other podping, gives a crawl
  when the window closes.
- A crawl that the node answers `no_change` doubles the window. An accepted
  change sets it to 30 seconds. The window is never more than 1 hour.
- A `live` podping inside a window gives a crawl at once.
- A podping for a URL that the skip list stops gives no crawl, and it does not
  change the window.
- A podping for a URL just after a follow fetch of that URL gives a crawl at
  once.
