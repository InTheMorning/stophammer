# ADR 0077: A New Body Value States Its Refill Pass

## Status
Accepted on 2026-10-22, for musicindex request 19. It amends the
"Consequences" section of ADR 0073.

## Date
2026-10-22

## Context

ADR 0073 made the parser keep each value time split. Release 0.13.0 deployed it
on 2026-10-21. After the deploy, the track "Autumn Rust, Boost Recap"
(`f0e12a75-5ec9-4abb-81d9-f918eeb474bc`) gave no split. Its RSS file has 8.

Two steps stop a body that did not change before the new parser reads it:

- The crawler sends nothing to the node when the feed server answers `304`
  (ADR 0050 §3).
- When the crawler sends a body, the `content_hash` verifier compares its hash
  with the hash that the node recorded last. When the two are equal, the node
  gives `NO_CHANGE` and writes no track (archived ADR 0072 §4).

The body of "Autumn Rust" did not change after 2026-10-02. So the node did not
read its splits. A podping does not correct this. A podping makes the crawler
fetch the feed, but the body and its hash stay the same.

On 2026-10-21, the operator searched the bodies in the crawler fetch cache for
the text `valueTimeSplit`. Six feeds matched. Four of them have splits: 38 in
total. Two only name the tag in their description. A `feed` pass with `--force`
read the six feeds again. The live API then gave each of the 38 splits.

The archived ADR 0058 §1c states a pass of this type for its own values:
"after the deploy the operator runs one `refresh` pass with
`--no-revalidate`". Before this decision, ADR 0073 stated no pass, and no
rule told an ADR to state one.

## Decision

### 1. The ADR states the pass

This section applies to an ADR that makes the parser or the node keep a new
value from a feed body. A new value is a value that the index did not keep
before. The ADR states a refill pass in its "Consequences" section. The pass
gives:

- the crawler mode and its flags. `--force` is necessary when the node can hold
  the same hash for the feed,
- the feeds that the pass reads: each feed, or each feed with the new tag in
  its body in the fetch cache,
- the check that shows the result on the live API.

An ADR that changes only the response of values that the index keeps now
needs no pass.

### 2. The pass is an open item until it runs

When a release deploys such an ADR, the "Open Items" section of `AGENTS.md`
names the pass. The item stays until the operator runs the pass and the check
of §1 gives the expected result.

### 3. The pass of ADR 0073

The `feed` pass of 2026-10-21 with `--force` is the refill pass of ADR 0073.
It read the six feeds of the fetch cache with the text `valueTimeSplit` in
their body.

## Alternatives Considered

### Correct only ADR 0073

This is the smallest change. But the next ADR that reads a new tag has the same
problem, and no record tells its author to examine it.

### Put the parser version in the hash

The node would then read each feed again at its next submission after a parser
change. But a `304` sends no submission, so a feed that does not change stays
unread. Also, each parser change would then read each feed, and most parser
changes add no value.

## Consequences

- Each future ADR of this type states its pass, and the operator runs it after
  the deploy.
- The search of 2026-10-21 covered only the fetch cache. A feed that is not in
  that cache was not searched. A forced `refresh` pass of each feed reads such a
  feed. It has not run.

## Checks

Manual, because a test cannot see a deploy or a pass of the operator:

- The review of an ADR that adds a value from a feed body checks that its
  "Consequences" section states the pass of §1.
- The release record of such an ADR checks that `AGENTS.md` names the pass
  in "Open Items" (§2).
