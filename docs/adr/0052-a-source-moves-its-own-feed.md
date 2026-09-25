# ADR 0052: A Source Moves Its Own Feed

## Status
Accepted on 2026-09-25

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
   at the same URL. On 2026-09-11 the Doerfelverse tool gave four Elijah Lied
   releases new channel GUIDs, and each release kept its item GUIDs. The new
   GUIDs stayed in each fetch for 14 days or more. On 2026-09-25 the
   publisher confirmed that the tool made them in error. No GUID, old or new,
   is the UUIDv5 of its feed URL.

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
| Self link | The body at the source URL declares `atom:link rel="self"` with the value `F` |
| Operator relocation | `PATCH /v1/feeds/{guid}` with the admin token, and a reason |

For the first three triggers, the node also needs all of these:

- The body at `F` declares the GUID of the record.
- `F` is not the source URL of a different record.
- `F` passes the fetch safety rules of ADR 0054.

A move changes `feeds.feed_url` to `F` and emits the signed `FeedUpserted`
event. These steps go in one
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

**Self link.** The parser already reads `atom:link rel="self"` as the link
type `self_feed`. The node records the value in `feeds.declared_self_url`, and
only an ingest in the update case of ADR 0051 writes that column. When a body
for `F` arrives in the mirror case, and `F` equals the recorded value, the node
moves the record and applies the content. This is the path of the new-feed
declaration, with a different element.

The node does not read the self link from `source_entity_links` for this
trigger. Before ADR 0051, a mirror body could write those rows, so a stored
link can come from a body that the source did not serve. The column starts
empty, and each record gets its value from its first source crawl after the
deploy.

**Relocation.** The operator relocation needs the admin token (ADR 0056).
This ADR adds one check to
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
- A redirect to the source URL from a different URL.

A publisher that lost the source URL uses the operator path. The operator
records the evidence in the reason field of the relocation.

### 4. The source URL can change its declared GUID

The source URL controls what the record declares. When the source URL
declares a new GUID that no record holds, ADR 0051 answers
`guid_change_pending`. This ADR decides what follows.

The namespace makes a GUID permanent. A change at the same URL is usually a
tool error, and the error can stay in each fetch for weeks. The Doerfelverse
case shows this. Thus a wait does not tell an error from an intended change.

**The pending row.** The node records the pending change: the source URL, the
old GUID, the new GUID, and the first and the last time it was seen. A table
holds at most one row for each source URL. If the source URL declares the old
GUID again, the node deletes the row. The record keeps its content, its GUID
and its track identities while the row exists.

**The automatic case.** The node makes the change at once when the new GUID
is the UUIDv5 of the source URL. The UUIDv5 is made as in ADR 0058 section
1b. The GUID was made for that URL, so the source corrects its GUID to the
form of the namespace. No other case is automatic.

**The operator case.** In each other case, the node makes the change only when
the operator approves it. `POST /v1/feeds/{guid}/guid-change` needs the admin
token. The body gives a decision, `approve` or `reject`, and a reason. The
node signs one `FeedGuidChangeDecided` event with the old GUID, the new GUID,
the decision and the reason.

- `approve` runs section 5 at the next submission of the new GUID from the
  source URL. The node does not keep the body of an earlier submission.
- `reject` keeps the record. The rejection holds while the source URL
  declares the same new GUID. A different new GUID makes a new pending row.

**The public list.** Each pending row is public, so a publisher can see the
error of a tool:

- `GET /v1/feeds/{guid}` adds `pending_guid_change` with the new GUID, the
  first and the last time seen, and the rejection when one holds.
- `GET /v1/guid-changes` lists each pending row with no rejection that holds,
  newest first, with the `QueryResponse` pagination.

The pending row and the decision replicate through signed events, so a
community node gives the same answer. `last_seen` is local to the primary,
as in ADR 0058.

### 5. A GUID change keeps a link to the old record

In one transaction, the node:

1. Retires the old record with the signed `FeedRetired` event and the reason
   `guid_superseded`. The retirement writes no block of ADR 0053. A block
   would stop each later submission from the source URL, and a revert would
   then be impossible.
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

The self-link trigger ships first, as its own phase. On 2026-09-24, 1,429
Wavlake records had a stored URL of the form `https://wavlake.com/feed/<id>`.
Each of their bodies names `https://wavlake.com/feed/music/<id>` as its self
link. A podping or a publisher link for the `music` form is a mirror under ADR
0051, and `refresh` runs only by hand. Thus those records get no update
between two manual passes until the trigger exists. The evidence is in
[the research record](../reviews/feed-removal-and-wavlake-forms-research.md).

The self-link phase needs no new event type and no parser change. It needs one
migration for `feeds.declared_self_url`. The check of ADR 0054 on `F` applies
when ADR 0054 exists.

- The parser adds `itunes:new-feed-url`. The ingest request adds
  `new_feed_url` and `redirects`. Both fields are optional, so an older
  crawler still works without moves.
- `FeedGuidChangeObserved`, `FeedGuidChangeDecided` and `FeedGuidSuperseded`
  are new event types. `FeedGuidChangeObserved` replicates the pending row. The rollout upgrades each
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
- A tool error that changes a GUID stays pending and public. The records keep
  their content and track identities. When the tool declares the old GUID
  again, the row goes away with no operator action. The Doerfelverse case
  follows this path.
- An intended GUID change that is not the UUIDv5 of the source URL needs the
  operator.
- A GUID change creates new track identities. A client that stored the old
  ones follows `superseded_by`.
- A compromised source host can move its feed. ADR 0056 keeps this limit of
  RSS control. This ADR does not change it.

## Invariants

- Only an ingest in the update case of ADR 0051 writes
  `feeds.declared_self_url`.
- The source URL changes only by a trigger in section 1.
- An unavailable source never releases its record.
- `superseded_by` never selects a payment route.
- A GUID change retires the old record with no block.
- A GUID change applies only when the new GUID is the UUIDv5 of the source
  URL, or when the operator approves it.

## Guards

A move and a GUID change have no incident in this index yet. The Doerfelverse
feed is the first GUID change. These guards earn a test because each one
protects a payment route:

- A `302` from the source URL applies content and keeps the source URL.
- A `301` from the source URL to a URL that declares a different GUID moves
  nothing.
- An `itunes:new-feed-url` value in a mirror body moves nothing.
- A new GUID seen in one fetch only changes nothing.
- A new GUID that is not the UUIDv5 of the source URL changes nothing, in any
  number of fetches over any time, until the operator approves it.
- A new GUID that is the UUIDv5 of the source URL applies on the first fetch.
- A return to the old GUID deletes the pending row, and the record keeps its
  track identities.
- A `reject` holds for the same new GUID, and a different new GUID opens a new
  row.
- A relocation to the source URL of a different record returns `409`.
- A self link in a mirror body moves nothing.
- A self link stored in `source_entity_links` before the deploy moves
  nothing. Only `feeds.declared_self_url` counts.

## Amendment Of 2026-09-25

Section 4 first applied a GUID change after the source URL declared it for 24
hours. The Doerfelverse error stayed for 14 days or more, so that rule would
have retired four correct records and made new track identities. The
publisher then corrects the tool, and the rule runs again in the other
direction. The amendment makes the change automatic only for the UUIDv5 of the
source URL, needs the operator in each other case, and makes pending changes
public.

### Alternatives considered for the amendment

- **A longer wait, for example 7 or 30 days.** The error lasted longer than 7
  days, and a wait gives no evidence of intent. Rejected.
- **Keep the old GUID as the identity, and record the new GUID as an alias.**
  The record keeps its track identities. A record could then differ from the
  GUID that its feed declares, and a second record could claim the alias. The
  alternative "Keep a stable internal identity" above covers the storage
  cost. Deferred until an intended change occurs.
