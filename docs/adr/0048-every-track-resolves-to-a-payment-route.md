# ADR 0048: Every Track Resolves To A Payment Route

## Status
Accepted

## Date
2026-09-23

## Context
`V4VPaymentVerifier` rejects a feed whose `feed_payment_routes` is empty. The
rule exists for a stated reason, recorded in the verifier's own documentation
and in the commit that added it on 2026-03-09: the feed-level block is the
fallback wallet, and a track that declares no routes of its own is paid from
it.

That reason justifies a narrower rule than the one written. A fallback is
needed only when a track has nothing of its own. A feed where each track
carries its own `podcast:value` block needs no fallback, because nothing falls
back. The verifier refuses that feed anyway, and it returns before the loop
that would have seen the track routes.

The Podcast Namespace allows `podcast:value` on an item. A feed that declares
a block on each track participates in value-for-value, and it can pay a
different split for each track, which a single channel-level block cannot.

No decision record owns the present rule. It is stated in a doc comment and a
commit message, so the unconditional form was never examined against its own
purpose. The same commit shows what examination looks like: it removed
`payment_route_sum` from the default chain after finding that V4V splits are
relative weights rather than percentages.

The corrective pass of ADR 0047 made the effect visible. Feeds the index
already holds are refused on re-ingest, so they keep the incorrect release date
that ADR 0043 exists to fix.

[The evidence record](../reviews/adr-0048-track-value-coverage-evidence.md)
holds the measurement. Four of four sampled refused feeds cover each track with
a valid item-level block. 227 of 7,491 accepted feeds hold a track that
declares no block, so the fallback stays necessary. The record does not give
the count of refused feeds, because the node does not store a refusal.

## Decision
The gate is coverage, not presence. Each track must resolve to at least one
valid payment route.

1. A track that declares its own routes is covered by them, and they must hold
   a recipient with a non-empty address and a positive split.
2. A track that declares no routes is covered by the feed-level routes, which
   must hold such a recipient.
3. A feed therefore needs a feed-level block only when at least one of its
   tracks declares none.
4. A feed with no tracks keeps the present requirement. There is nothing to
   cover, and a music feed with no track is not a participant.
5. The `publisher` and `musicL` exemption is unchanged. Those mediums are
   source-layer containers.
6. A failure names the track that resolves to no route, so an author can find
   it. The present message names only the feed.

## Alternatives Considered

### Keep the present rule

It is simple to state and it guarantees a fallback exists. It also refuses
feeds that pay every track correctly, which is the outcome the rule was written
to protect. Rejected.

### Accept any feed that has a route anywhere

This admits a feed where one track is payable and the rest are not. The reason
for the original rule was that each track is payable, and this abandons it.
Rejected.

## Consequences

- Feeds that declare payment on each track and none on the channel become
  acceptable. This widens what the index admits, which is why this decision
  needs the operator rather than a code change.
- Feeds already held that are refused on re-ingest for this reason become
  reachable by a corrective pass, so ADR 0043 can correct them.
- A feed that pays some tracks and not others is still refused, and the message
  now says which track.
- The verifier reads the track routes it already receives. No new data crosses
  the ingest boundary.

## Invariants

- Each track of an accepted feed resolves to at least one valid route.
- A feed-level block is required exactly when a track declares none.
- Medium exemptions stay with the medium, not with the payment shape.

## Guards

The present rule drifted from its purpose without anything reporting it. The
replacement earns tests for each branch.

- A feed whose tracks each carry a valid route, and which has no feed-level
  block, is accepted.
- A feed with one track that declares no routes, and no feed-level block, is
  refused, and the message names that track.
- A feed with a valid feed-level block and a track whose declared block has no
  valid recipient is refused. This is unchanged from the present rule.
- A feed with no tracks and no feed-level block is refused.
