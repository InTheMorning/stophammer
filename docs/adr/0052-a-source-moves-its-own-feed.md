# ADR 0052: A Source Moves Its Own Feed

## Status
Proposed

## Date
2026-09-24

## Context
ADR 0051 makes the stored `feed_url` the source URL of a record. Only content
from the source URL changes the record. That rule stops a takeover, but it
also stops two honest changes:

1. **A move.** The publisher moves the feed to a new host and keeps the
   `podcast:guid`. The namespace requires that the GUID stays the same
   across a move.
2. **A GUID change.** The publisher tool starts to declare a different GUID
   at the same URL. On 2026-09-24 a Doerfelverse feed changed its channel
   GUID and kept its five item GUIDs.

Today no crate reads `itunes:new-feed-url` or `podcast:locked`. The crawler
follows redirects with the reqwest default policy. It reports the first and
the last URL, but not the status of each redirect.

The evidence in
[the conflict evidence](../reviews/feed-identity-conflict-evidence.md) shows
1,070 April 2026 URLs that are not in the index while their GUID is at a
different URL. 1,051 of them redirected. Moves are frequent.

The common migration procedure uses two signals from the old location: a
permanent redirect, and `itunes:new-feed-url`. In that procedure, the new URL
does not move a feed by its own claim. The complete live algorithm of Podcast
Index is not known. See
[the Podcast Index evidence](../reviews/feed-identity-podcastindex-evidence.md).

An unavailable source does not give authority to a new URL. HTTP `404`,
`410`, a DNS failure and a timeout name no successor.

## Decision

### 1. Only the source URL can start a move

The source URL of a record changes only by one of these:

| Trigger | Evidence the node needs |
|---|---|
| Permanent redirect | A fetch of the source URL follows only `301` or `308` answers to a final URL `F` |
| New-feed declaration | The body at the source URL declares `itunes:new-feed-url` with the value `F` |
| Publisher relocation | ADR 0018 proof at the source URL, then `PATCH /v1/feeds/{guid}` |
| Operator relocation | `PATCH /v1/feeds/{guid}` with the admin token, and a reason |

For the first two triggers, the node also needs all of these:

- The body at `F` declares the GUID of the record.
- `F` is not the source URL of a different record.
- `F` passes the fetch safety rules of ADR 0054.

A move changes `feeds.feed_url` to `F`, revokes the proof tokens of the
record, and emits the signed `FeedUpserted` event. These steps go in one
transaction. The old URL keeps its URL observation under ADR 0049.

After the move, the old URL is a mirror under ADR 0051. Content from it does
not change the record. A move back needs a new trigger at `F`.

### 2. How each trigger reaches the node

**Permanent redirect.** The crawler records each redirect hop as a URL and a
status. The ingest request adds a `redirects` list. The node moves the record
to `canonical_url` when two conditions are true:

- `source_url` is the source URL of the record.
- Each hop is `301` or `308`.

The node applies the content in the same transaction as the move. A `302` or
`307` hop is delivery only. The node applies the content and keeps the source
URL.

**New-feed declaration.** The parser reads `itunes:new-feed-url` into a typed
field. The node stores the value on the record, from the source URL body
only. The crawler adds the value to its follow list. When the body at `F`
arrives, the node finds the mirror case of ADR 0051. If `F` is the stored
new-feed value of the record, the node moves the record and applies the
content.

Thus neither trigger needs a new fetch by the node. The source URL itself
gave the evidence, in a body that only the source can change.

**Relocation.** ADR 0018 owns the proof. This ADR adds one check to
`PATCH /v1/feeds/{guid}`: the new URL must not be the source URL of a
different record. The handler answers `409` with `record_conflict`.

### 3. Signals that do not move a feed

- `podcast:locked` and its `owner` email. The node stores them as source
  facts. They control import between hosting platforms. They grant nothing in
  this index.
- A `404`, a `410`, a DNS failure, a TLS failure or a timeout at the source
  URL, for any time.
- The same item GUIDs, titles, audio, images or payment addresses at a
  different URL.
- ADR 0018 proof at the new URL only.
- A redirect to the source URL from a different URL.

A publisher that lost the source URL uses the operator path. The operator
records the evidence in the reason field of the relocation.

### 4. The source URL can change its declared GUID

The source URL controls what the record declares. When the source URL
declares a new GUID that no record holds, ADR 0051 answers
`guid_change_pending`. This ADR decides what follows.

The node records the pending change: the source URL, the old GUID, the new
GUID, the first time and the last time it was seen. A table holds at most one
row for each source URL.

The node makes the change when one of these is true:

- The source URL declared the same new GUID in two fetches with 24 hours or
  more between them, and in each fetch between them.
- The publisher completes ADR 0018 proof at the source URL.
- The operator approves the change with the admin token.

A tool error that gives a wrong GUID for one fetch does not pass the first
condition. If the source URL returns to the old GUID, the node deletes the
pending row.

### 5. A GUID change keeps a link to the old record

In one transaction, the node:

1. Retires the old record with the signed `FeedRetired` event and the reason
   `guid_superseded`.
2. Records the link from the old GUID to the new GUID with a new signed event,
   `FeedGuidSuperseded`.
3. Admits the new record with the source URL, from the body that gave the new
   GUID.

The link is for navigation only. `GET /v1/feeds/{old_guid}` answers `404`
with a `superseded_by` field. The API does not use the link to resolve a
payment route, a track or a publisher relationship. A client that stored an
old track identity uses the link to find the new record, and then chooses.

ADR 0040 makes track identity feed-scoped, so each track gets a new identity.
The link does not map old tracks to new tracks.

If the source URL later declares the old GUID again, the same process runs in
the other direction. The link of the first change stays in the signed
history.

### 6. Rollout

- The parser adds `itunes:new-feed-url`. The ingest request adds
  `new_feed_url` and `redirects`. Both fields are optional, so an older
  crawler still works without moves.
- `FeedGuidSuperseded` is a new event type. The rollout upgrades each
  community node before the primary emits one. An older community node cannot
  parse a type that it does not know.
- ADR 0044 requires the new response field in the OpenAPI document.

## Alternatives Considered

### Move when the new URL declares the GUID and the old URL is down
This is rule H of the evidence record. It lets an attacker take any feed
whose host fails. Rejected.

### Keep a stable internal identity beside the declared GUID
This preserves the record through a GUID change without a new record. It
changes the primary key that about 15 tables use, and each track identity
under ADR 0040. It is a large storage change for a rare case. Rejected for
now. A future ADR can take it if GUID changes become frequent.

### Forward payments from the old GUID to the new GUID
A payment client would then pay a record that it did not select. Two releases
can share an old GUID by accident. Rejected.

### Wait for two redirects before a move
A `301` is a statement of the source server that the move is permanent. The
body at `F` must also declare the same GUID, and that check stops a wrong
redirect to a different feed. A second wait adds delay and no evidence.
Rejected.

## Consequences

- A host move with a `301` or `itunes:new-feed-url` updates the index on the
  first crawl after the move.
- A publisher that loses the old host needs the operator. This is the cost of
  a rule that an attacker cannot use.
- The Doerfelverse case resolves after 24 hours with no operator action.
- A GUID change creates new track identities. A client that stored the old
  ones follows `superseded_by`.
- A compromised source host can move its feed. ADR 0018 records this limit of
  RSS control. This ADR does not change it.

## Invariants

- The source URL changes only by a trigger in section 1.
- An unavailable source never releases its record.
- `superseded_by` never selects a payment route.

## Guards

A move and a GUID change have no incident in this index yet. The Doerfelverse
feed is the first GUID change. These guards earn a test because each one
protects a payment route:

- A `302` from the source URL applies content and keeps the source URL.
- A `301` from the source URL to a URL that declares a different GUID moves
  nothing.
- An `itunes:new-feed-url` value in a mirror body moves nothing.
- A new GUID seen in one fetch only changes nothing.
- A relocation to the source URL of a different record returns `409`.
