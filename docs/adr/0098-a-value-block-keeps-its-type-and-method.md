# ADR 0098: A Value Block Keeps Its Type And Its Method

## Status
Accepted on 2026-11-14. Release 0.21.0 of 2026-11-17 holds it, and its refill
pass ran on 2026-11-17. It answers
[the research request of 2026-11-13](../plans/research-value-block-type-request.md),
V1 to V4 and its three questions.

- Revised: 2026-11-14. The search for the code locations found five points
  that the text did not state. §1 says that only an item has a value time
  split. §3 gives the sum check of each block. §4 gives the stored rows with
  no block, the block with only splits and the routes that a track gets from
  its feed. §5 gives the event for a change of only the routes of a track. §6
  says that the pass can read the cached body.

## Date
2026-11-13

## Context

A feed can have more than one `podcast:value` block. Each block has a `type`
and a `method`. On 2026-11-13 a proposal for payments in USDC on Base added
the block type `usdc`, the method `eip3009` and the recipient type `address`.
The address has the form `eip155:8453:0x…`. The research note is
`research_notes/wallets/usdc-on-base.md` in `~/build/musicindex-research`,
commit `e69bfb9`.

The main session examined stophammer-parser at commit `aa834a4`:

- `extract_payment_routes` (`src/engine.rs:634`) puts the recipients of each
  `podcast:value` block in one list. It does not read the `type` or the
  `method` of a block.
- `read_payment_recipient` (`src/engine.rs:597`) gives `RouteType::Lnaddress`
  for `type="lnaddress"` and `RouteType::Node` for each other value. The
  parser `RouteType` has only these two values.
- `extract_value_time_splits` reads the recipients of a local split with
  `read_payment_recipient`. It does not keep the type of the parent block.

In the node, `model::RouteType` (`src/model.rs:147`) has the four values
`node`, `wallet`, `keysend` and `lnaddress`. A community node cannot decode an
event payload with a different value. `route_type_from_db` (`src/db.rs:1207`)
changes an unknown stored value into `node`. The tables `payment_routes`,
`feed_payment_routes`, `value_time_split_recipients` and `feed_list_value_raw`
accept only the four values.

Thus a USDC recipient becomes a Lightning `node` route with an `eip155:`
address. A feed with a Lightning block and a USDC block gives one route list
with the splits of the two blocks. A client can then send a keysend payment to
a Base address.

The main session read `backups/stophammer-20261113T123627Z.db` with SQLite in
immutable mode. It counted the `node` rows with an address that is not 66
hexadecimal characters:

| Table | Rows | `eip155:` address | Other `node` address | Different addresses |
|---|---:|---:|---:|---:|
| `payment_routes` | 33,268 | 0 | 48 | 7 |
| `feed_payment_routes` | 10,884 | 0 | 12 | 7 |
| `value_time_split_recipients` | 410 | 0 | 0 | 0 |
| `feed_list_value_raw` | 15 | 0 | 0 | 0 |

The request gives 35 and 7 rows. The main session did not find the rule that
gives these numbers. The addresses include `https://paypal.me/carlosbacalao`,
Lightning addresses such as `steven@getalby.com`, a 64-character hexadecimal
value and a 67-character placeholder value. The feed text of these rows is not
examined. The index has no USDC route today.

ADR 0073 §3 and §4 give the pattern for a change of the event payload and of
the `v1` response. ADR 0044 §4 makes a rename or a removal in `v1` a breaking
change. ADR 0077 states that a new value from a feed body needs a refill pass.

## Decision

### 1. The parser reads each value block with its type and its method (V1)

For each `podcast:value` of a channel or an item, the parser records:

- `value_type`: the `type` attribute, trimmed and in lowercase. A missing or
  empty attribute gives `lightning`.
- `value_method`: the `method` attribute, trimmed and in lowercase. A missing
  or empty attribute gives `None`.
- `block_index`: the position of the block in the `podcast:value` blocks of
  the same channel or item, from 0.

Each recipient and each value time split keeps the three values of its block.
The parser keeps a block of each type. It skips no block because Stophammer
does not know its type. A block with no recipient and no split gives no row.

Only an item and a live item have a value time split. The parser reads no
split of a channel, as before this ADR.

### 2. A recipient keeps the type that the feed gives (V2)

The parser keeps the `type` of a recipient, trimmed and in lowercase. A
missing or empty `type` gives `node`, as today.

The parser `RouteType` and the node `model::RouteType` keep their known values
and get one value that holds each other text. The two types write that text and
read the same text. `route_type_from_db` does not change an
unknown value into `node`. A value that Stophammer does not know is not changed
into `node` in a step.

A migration removes the `CHECK` on `route_type` in the four route tables. It
adds the columns `value_type`, `value_method` and `block_index` to these
tables and to `value_time_splits`. Each existing row gets `NULL` in the three
columns. `NULL` means that the row was stored before this ADR.

### 3. The routes of two blocks stay apart (V3)

The store keeps the `block_index` of each route and each value time split. A
reader of the store groups the routes of one channel or one item by
`block_index`. The split of a recipient applies only in its block.

A value time split is a child of one `podcast:value` block. The split and the
recipients of a local split get the type, the method and the index of that
block. This answers question 3.

The optional verifier `payment_route_sum` adds the splits of each block of a
track, not the splits of all its blocks. A block passes when its sum is 100.

### 4. The API gives the blocks and keeps the old lists safe (V4)

The include `payment_routes` of a feed and of a track keeps each field that it
has, with the same name and the same type. It gives a route only when it
agrees with all these conditions:

- The route is in a block with `value_type` `lightning`, or the row has
  `value_type` `NULL`.
- The route has `route_type` `node` or `lnaddress`.
- A `node` route has an address of 66 hexadecimal characters.

A track with no stored route gets the routes of its feed, as before this ADR.
A track with a stored route does not get them, also when the conditions remove
each of its routes. The conditions apply to the routes of the feed too.

A client that reads only `payment_routes` thus gets no USDC route and no route
of an unknown type. It also gets no `node` route that is not a public key. The third
condition also removes the 60 stored rows of §Context before the refill pass
of §6.

A new include `value_blocks` gives each block in source order, with
`block_index`, `value_type`, `value_method` and its recipients. Each recipient
has the fields of a route in `payment_routes`, and `route_type` is the text
that the feed gives. It applies no filter of this section.

`value_blocks` reads the same rows as `payment_routes`. A track with no stored
route gets the blocks of its feed. The stored rows with `NULL` in the block
columns give one block, with `null` in `block_index`, `value_type` and
`value_method`. That block is the first one. A block with only value time
splits is not in `value_blocks`. The split includes give its splits with the
block fields.

The includes
`value_time_splits` and `local_value_time_splits` add the fields
`block_index`, `value_type` and `value_method`. The OpenAPI document declares
each new name (ADR 0071 §1c).

### 5. The event payload carries the block fields

This section applies to the route records of the event payloads
`TrackUpserted`, `RoutesReplaced`, `FeedRoutesReplaced` and
`FeedListValueReplaced`, and to the value time split records. These records add
the optional fields `value_type`, `value_method` and `block_index`. A node applies an event that a node signed before this ADR.
Missing fields give `NULL`. A `route_type` of `wallet` or `keysend` in an
event from before this ADR keeps its value. Community nodes upgrade before the primary (ADR 0066).

On 2026-11-14, `build_changed_events` in `src/db.rs` gave a `TrackUpserted`
event only when a field of the track changed. A change of only the routes or
the value time splits of a track gave no event, and a community node kept the
old rows. An ingest now gives a `TrackUpserted` event when the routes or the
value time splits of a known track change. The block fields are part of this
comparison.

### 6. A refill pass replaces the rows that the parser changed (question 2)

The node does not change a stored row from a guess. A forced `feed` pass reads
the feed again and replaces its routes with the rule of this ADR (ADR 0077).
After a `304` the crawler parses the body in its fetch cache, and that is
sufficient for this pass.
The refill pass of this ADR is a `feed` pass with `--force` of these feeds:

- Each feed with a `node` row in a route table with an address that is not 66
  hexadecimal characters.
- Each feed with a `podcast:value` that is not `type="lightning"` in its body
  in the fetch cache.

The other feeds keep `NULL` in the block columns until their body changes.
`NULL` rows of these feeds are Lightning routes, and §4 gives them as today.

The pass record gives the counts of §Context before and after the pass. The
check on the live API uses one feed of the first set:

- `include=value_blocks` gives the recipient with the `route_type` of its feed.
- `include=payment_routes` does not give that recipient when its type is not
  `node` or `lnaddress`.

### 7. Not in scope

- Payment code in a client, and USDC payments.
- The `suggested` attribute of a block. A different decision can add it.
- A check of an address in `value_blocks`. The node keeps what the feed gives.
  Only `payment_routes` applies the check of §4.

## Alternatives Considered

### Skip each block that is not `lightning`

This needs a smaller change and keeps all lists safe. But the index then does
not show the data that the feed gives, and a subsequent client for USDC has no
source. The research recommends storage.

### Add the block fields to `payment_routes` and give all routes there

This needs no new include. But a client that reads only `payment_routes` today
gets USDC recipients and mixed splits. That is the defect of this ADR.

### A separate table for value blocks

A table with one row for each block holds the type and the method one time,
and it keeps a block with no recipient. The route rows then must have a key to
the block, and the event payloads must have a new record. Three columns on each
route row and each split row need fewer changes. A block with no recipient and
no split has nothing to pay, so §1 does not keep it.

## Consequences

- The change touches the parser, the store, the event payload, the apply step
  of a community node, the API and the OpenAPI document.
- Community nodes upgrade before the primary.
- A feed body that did not change after the deploy keeps its old routes until
  the refill pass of §6 reads it.
- No `v1` field of `payment_routes`, `value_time_splits` or
  `local_value_time_splits` changes its name or its type.
- `payment_routes` gives fewer rows than before: the 60 stored rows of
  §Context, and each route of a block that is not `lightning`.
- HitIt and the website get no USDC route from `payment_routes`. To show or
  pay a USDC recipient, a client reads `value_blocks`.

## Guards

Each guard is mechanical.

- Parser tests: a feed with a Lightning block and a USDC block gives two
  blocks with their types, methods and splits. A recipient with
  `type="address"` keeps `address`. A value time split in a USDC block gets
  the type `usdc`.
- A migration test: an existing row of each route table stays, with the three
  new columns `NULL`.
- Node tests: an event payload from before this ADR applies. A payload with
  `route_type` `address` applies on a community node and reads back as
  `address`. A change of only the routes of a known track gives a
  `TrackUpserted` event. The verifier `payment_route_sum` passes two blocks
  that each have the sum 100.
- Contract tests: `payment_routes` gives no route of a USDC block, no route
  with an unknown `route_type` and no `node` route that is not 66 hexadecimal
  characters. `value_blocks` gives each block in source order, and the rows
  with `NULL` block columns as one block.
