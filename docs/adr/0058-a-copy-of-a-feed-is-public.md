# ADR 0058: A Copy Of A Feed Is Public

## Status
Accepted on 2026-09-25

## Date
2026-09-25

## Context
Since ADR 0051, only the stored source URL of a record changes that record. A
submission from a different URL with the same `podcast:guid` is a mirror. The
node records the URL in `feed_url_observations`, logs `source_conflict`, and
discards the body.

A mirror is one of these:

- **An alias.** The same content at a second URL, for example the two URL
  forms of Wavlake, or a host name that redirects.
- **A copy.** A different feed that declares the same GUID, with different
  tracks or different payment routes. An unfinished move to a new host makes
  a copy. An impersonation also makes a copy.

The node cannot tell a move from an impersonation by the feed text.

The album `7192ec54-3aa2-5c61-987b-51bf75f68568` is a copy. Wavlake made the
GUID in December 2025, as a UUIDv5 of its own URL. A copy at
`musicsideproject.com` declares the same GUID and the same item GUIDs, and it
pays a different address. The node saw the second host first. Before ADR
0051, the record changed between the two payment routes six times from June
to September 2026.

At this time the record stays on the second host, and no client can see that
the Wavlake copy exists. `AGENTS.md` mandate 3 says that the index shows a
conflict to the operator and does not decide it silently.

`feed_url_observations` holds a URL, a GUID and a time. It holds no content,
so it cannot tell an alias from a copy. In the backup of 2026-09-24, 1,619
records had an observation at a URL that is not the source. Of these, 1,407
were Wavlake, and most of those are aliases.

The admin relocation of ADR 0052, `PATCH /v1/feeds/{guid}` with `feed_url`,
changes only the URL. It keeps the stored `last_build_date`, so the stale rule
of ADR 0053 section 3 can reject the first body from the new URL. It also
keeps `declared_self_url`, so a submission from the old URL can move the
record back by ADR 0052 section 2.

## Decision

### 1. The node records a summary of each mirror body

For each submission that ADR 0051 classifies as a mirror, the node reads the
parsed body before it discards it. It makes a summary:

- the channel title,
- the ordered item GUIDs,
- the recipient set of the feed, and the recipient set of each item. A
  recipient set is the set of ADR 0053 section 4, with the keysend custom
  record.

The node stores the summary in a new table, `feed_copies`, with one row for
each pair of GUID and URL. The row also holds the first time the node saw that
URL for that GUID.

The node writes a signed `FeedCopyObserved` event only when the summary of a
pair is new or changes. Each node applies the event, so a community node gives
the same answer. The per-request `lastBuildDate` of Wavlake is not in the
summary, so it does not make an event.

The primary also keeps `last_seen`, the time of the last submission of the
pair. That time is local to the primary and makes no event. A community node
returns `last_seen` as null.

### 1a. The number of rows for one GUID has a limit

A GUID has at most 20 rows. When a mirror body arrives for a new URL and the
GUID has 20 rows, the node writes no row and no event. It adds one to a
counter, `copies_over_limit`, which is local to the primary. A URL block of
ADR 0053 deletes the rows of that URL, so a block makes space again.

An attacker can fill the 20 rows before the real feed arrives. The counter
then shows the attack, and the record is on the list of section 3. The
operator blocks the attacker URLs, and the next crawl of the real feed adds
its row.

### 1b. A row can name the origin of the GUID

The namespace makes a `podcast:guid` as the UUIDv5 of the feed URL with no
scheme. When the GUID is the UUIDv5 of the URL of a row, and is not the
UUIDv5 of the source URL, the row gets `guid_origin: true`. The GUID was made
for the URL of that row, not for the source URL.

The flag is evidence for the operator, not a rule. A correct move to a new
host keeps the old GUID, so the old URL also gets the flag. The flag can only
be true for a URL that the node has seen.

In the backup of 2026-09-24, 6,821 of 10,410 GUIDs were the UUIDv5 of their
own source URL. Three records had a copy at the origin URL of the GUID while
the source URL was a different feed: `7192ec54`, `27735a10` and `c17b43a2`.
The other matches were the two URL forms of Wavlake, which are aliases.

### 2. A copy is a row that differs from the record

The node compares a row with the current record when a client reads it:

- `differs_tracks`: the item GUIDs are not the same set.
- `differs_recipients`: a recipient set of the feed or of a shared item is not
  the same.

A row with no difference is an alias. A row with one or two differences
is a copy. The title is information only.

### 3. The API shows each copy

- `GET /v1/feeds/{guid}` adds `copy_count`: the number of open copies (section
  4).
- `GET /v1/feeds/{guid}/copies` returns each row of the record: the URL, the
  first and last time seen, the title, the two differences, `guid_origin`,
  the recipient sets, and the resolution when one exists. On the primary it
  also returns `copies_over_limit`. An alias is in the list, with both
  differences false.
- `GET /v1/copies` returns each record with one or more open copies, newest
  first, with the `QueryResponse` pagination. On the primary it also returns
  each record with a `copies_over_limit` above zero.

The routes are public and in `query_routes`, so a community node serves them.
A row at a URL with an ADR 0053 block is not in any answer.

### 4. The operator resolves a copy

`POST /v1/feeds/{guid}/copies/resolve` needs the admin token. The body gives
the URL of the row, a decision and a reason:

| Decision | Result |
|---|---|
| `keep_source` | The record stays. The copy is resolved. |
| `relocate` | The row URL becomes the source URL of the record, in the same transaction as section 5. The old source URL becomes a row when the node next sees it. |

The node signs one `FeedCopyResolved` event with the GUID, the URL, the
decision, the reason, the time and a digest of the summary. The resolution
holds while the summary of the row has the same digest. When the copy
changes, the row is open again.

A copy is **open** when it differs and has no resolution that holds.

To reject each later submission from the URL, the operator adds a URL block of
ADR 0053. This ADR adds no third decision for it.

The largest risk of this section is a false request to the operator. A person
can show a copy with a plausible story. An example is "this is the new
official host". Before a `relocate`, the operator examines these items and writes the
result in the reason:

1. The new URL declares the same GUID now. The operator fetches it.
2. No other record has the new URL as its source URL.
3. The evidence agrees with a move: a redirect or `itunes:new-feed-url` at
   the old URL, the self link of the new URL, `guid_origin`, and the first
   time the node saw each URL.
4. The payment recipients of the new URL belong to the same artist. The
   operator gets this from the artist through a channel that the requester
   does not control.

When an item fails, the operator does not relocate. `keep_source` is the
safe decision, because it changes no payment route.

### 5. A relocation clears the fields that belong to the old source

Each relocation clears `last_build_date` and `declared_self_url` of the
record. This applies to `relocate` and to `PATCH /v1/feeds/{guid}` with
`feed_url`. The next body from the new source URL applies with no stale rule
and records its own self link. The relocation requires a reason, and the
signed `FeedUpserted` event of the relocation carries it.

## Alternatives Considered

### Show the observed URLs only
A list of URLs cannot tell an alias from a copy. Such a list shows about
1,400 Wavlake aliases as conflicts. Rejected.

### Keep the whole mirror body
The body is large, and the node signs and replicates each event. The summary
holds what a client needs to see the difference: the tracks and the payment
routes. Rejected.

### Compare content hashes
Wavlake writes the request time in `lastBuildDate`, so each fetch has a new
hash. Rejected.

### Select the source by the UUIDv5 of the GUID
The GUID of the example album is the UUIDv5 of the Wavlake URL. But a correct
move to a new host keeps the old GUID, so the rule would reject each correct
move. The operator decides with the evidence. Rejected.

### Show copies to the admin only
A client that shows the album cannot tell the listener that a second copy
with different payment exists. The index shows its conflicts. Rejected.

## Consequences

- A client can show "also published at" with the difference, for each album.
- The operator has one list of open conflicts, and one route to decide each.
- An impersonation attempt is public. The page of the victim shows the URL of
  the attacker until the operator blocks it.
- One new table, one new migration and two new event types. Community nodes
  get the version before the primary emits the events.
- An attacker can fill the 20 rows of a GUID and delay the real copy until
  the operator blocks the attacker URLs. The counter shows the attack.
- The admin relocation that exists now clears two fields, and it needs a
  reason.

## Invariants

- A mirror body never changes the record. This ADR adds a summary row only.
- A relocation clears `last_build_date` and `declared_self_url` in the same
  transaction.
- A resolution holds only for the summary digest it names.
- A GUID has at most 20 rows.
- Only a new or changed summary makes a `FeedCopyObserved` event.
- Each public answer excludes a URL with an ADR 0053 block.

## Non-Goals

- The `record_conflict` and `guid_change_pending` cases of ADR 0051. ADR 0052
  section 4 owns the GUID change and its public list.
- An automatic choice between two copies.
- A fetch by the node. The summary comes from what the crawler submits.

## Guards

The incident is the album `7192ec54-3aa2-5c61-987b-51bf75f68568`: its payment
route changed six times, and no client could see the second copy. Tests:

- A mirror body with different recipients gives a row with
  `differs_recipients`, and `GET /v1/copies` lists the record.
- A mirror body with the same tracks and recipients gives an alias, and
  `GET /v1/copies` does not list the record.
- A `keep_source` resolution closes the copy, and a change of the copy opens
  it again.
- A `relocate` resolution sets the source URL, clears `last_build_date` and
  `declared_self_url`, and the next body from the new URL is an update.
- After a relocation, a submission from the old URL does not move the record
  back.
- A community node that applies the events gives the same answer from each
  route, except `last_seen` and `copies_over_limit`.
- A mirror body with the same summary as the stored row makes no event.
- With 20 rows for a GUID, a mirror body from a new URL makes no row and no
  event, and `copies_over_limit` increases. A URL block deletes the row of
  that URL.
- A copy at the URL whose UUIDv5 is the GUID gets `guid_origin: true`. A copy
  at another URL gets `false`.
