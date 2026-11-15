# Research Request: Keep The Payment Method Of Each Value Block

## Status

Answered by [ADR 0098](../adr/0098-a-value-block-keeps-its-type-and-method.md), Accepted - 2026-11-14.

| Section | Status | Owner or blocker |
|---|---|---|
| V1, record the type and the method of each value block | Accepted, not released | ADR 0098 |
| V2, keep the recipient type that the feed gives | Accepted, not released | ADR 0098 |
| V3, keep the routes of two value blocks apart | Accepted, not released | ADR 0098 |
| V4, give the method to API clients | Accepted, not released | ADR 0098 |

This request comes from the research repository at
`~/build/musicindex-research`, commit `e69bfb9`. That repository has no Git
remote. Its index is `AGENTS.md`. The note is
`research_notes/wallets/usdc-on-base.md` in that repository.

This document binds nothing in Stophammer. It asks Stophammer to examine one
change and to make a decision. Stophammer records the decision in its own ADR.

The data is in `sources/captures/2026-11-13-v4v-usdc/` in the research
repository. `SHA256SUMS` in that folder gives the hashes. The research examined
stophammer-parser at commit `aa834a4` and the backup
`backups/stophammer-20261113T123627Z.db`.

## Background

On 2026-11-13 Adam Curry published a proposal for value for value payments in
USDC on Base. USDC is a dollar stablecoin. Base is the Ethereum layer-2 network
from Coinbase. The page is `https://currycaster.com/v4v-usdc/`.

A feed names a USDC recipient with the same `podcast:value` tag and new
attribute values:

```xml
<podcast:value type="usdc" method="eip3009" suggested="0.00005">
  <podcast:valueRecipient name="The show" type="address" address="eip155:8453:0x6DB7be2ec8c97e1f84849c62A9e865647677e790" split="99"/>
  <podcast:valueRecipient name="Split partner" type="address" address="eip155:8453:0x91c400e92A11feABFB545B29C09e2fDD1b0ED8e2" split="1"/>
</podcast:value>
```

The test feed is `https://mp3s.nashownotes.com/v4v-usdc-test/feed.xml`. A
first boost with this feed is on Base in transaction
`0x85ead4e3bb8a7636ab3d71e0caffb947d7ca85c992fa3507f55ca39ff8fd7449`.
The values `usdc`, `eip3009` and `address` are not in the Podcast Namespace on
2026-11-13. The namespace lets a feed have more than one
`podcast:value` block, each with a different `type`.

## The Defect

At parser commit `aa834a4`:

- `extract_payment_routes` (`src/engine.rs:634`) reads each recipient of each
  `podcast:value` block. It does not read the `type` or the `method` of the
  block.
- `read_payment_recipient` (`src/engine.rs:605`) sets the route type to
  `Lnaddress` for `type="lnaddress"`. It sets each other value to `Node`.

The `route_type` column of `payment_routes` and `feed_payment_routes` accepts
only `node`, `wallet`, `keysend` and `lnaddress`.

Inference: the parser stores each USDC recipient of the test feed as a Lightning
`node` route with an `eip155:` address. If a feed has a Lightning block and a
USDC block, the parser puts the recipients of the two blocks in one list. The
splits of the two blocks are then mixed, and a client can try a keysend payment
to a Base address.

## Data In The Index

Source: `backups/stophammer-20261113T123627Z.db`, read with SQLite in immutable
mode.

| Table | Rows | `eip155:` address | `node` route with an address that is not a Lightning public key |
|---|---:|---:|---:|
| `payment_routes` | 33,268 | 0 | 35 |
| `feed_payment_routes` | 10,884 | 0 | 7 |

The 7 `feed_payment_routes` rows include `https://paypal.me/carlosbacalao`,
four Lightning addresses such as `steven@getalby.com`, and a 64-character hex
value. The research did not examine the source feeds of these rows. The index
has no USDC route today.

## Purpose

| # | Change | Evidence |
|---|---|---|
| V1 | Record the `type` and the `method` of each `podcast:value` block | The parser does not read the two attributes |
| V2 | Keep the recipient `type` that the feed gives. Do not change a type that Stophammer does not know into `node` | 42 stored `node` rows have an address that is not a Lightning public key |
| V3 | Keep the routes of two value blocks in the same feed or item apart, with the split of each block | One route list mixes the blocks |
| V4 | Give the block type and method in the API. A client then selects only the routes that it can pay | HitIt and the website get the routes from the API |

## Questions For Stophammer

1. Does Stophammer store a value block that is not `lightning`, or skip it?
   The research recommends that it stores the block with its type and method.
   The index then shows the data that the feed gives.
2. What occurs to the 42 stored `node` rows with a different address?
3. Does the change also apply to the recipients of a value time split?
   `read_payment_recipient` serves the two.

## Not In Scope

- This request asks for no payment code. It asks for no USDC payments in a client.
- The research did not read the HitIt payment code. A different request can
  find how HitIt acts on a route with an `eip155:` address.
