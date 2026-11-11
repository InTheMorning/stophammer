# Research Request: A Feed Move Follows A Chain And Refuses A Loop

## Status

Answered in part - 2026-11-11. ADR 0083 answers M1 and M3. M2 is open, and ADR 0095 is Proposed for it.

| Section | Status | Owner or blocker |
|---|---|---|
| The Defect | Answered | [ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md) §2 and §5. Release 0.18.1 holds it |
| M1: The Chain Rule | Answered with a different rule | ADR 0083 §2 to §8. "Alternatives Considered" gives the reason to refuse the chain rule |
| M2: Two Feeds That Name Themselves | Open | [ADR 0095](../adr/0095-a-release-on-two-platforms-is-two-feeds.md) §4 is Proposed: the copy row does not store the tag of a mirror. It stays open until the operator accepts it |
| M3: Spelling | Answered with no change | ADR 0083 §9. The comparison stays exact |

Received - 2026-11-04. This request comes from the research repository at
`~/build/musicindex-research`. That repository has no Git remote. Its index is
`AGENTS.md`. The note is `research_notes/aggregation/itunes-new-feed-url.md`
in that repository.

This document binds nothing in Stophammer. It asks Stophammer to examine three
changes and to make a decision. Stophammer records each decision in its own
ADR. Change M1 amends ADR 0052.

The data is in `sources/captures/2026-11-04-itunes-new-feed-url/` in the
research repository. `SHA256SUMS` in that folder gives the hashes. The probe
and the scratch patch are in `tools/itunes-new-feed-url/` in that repository.

The research examined Stophammer at commit `d807f8b`, the parser at commit
`226e390` and the crawler at commit `cb893cb`. It also examined the backup
`backups/stophammer-20261031T234835Z.db` and the PodcastIndex snapshot of
2026-10-31.

Stophammer examined this request on 2026-11-04 and corrected it. Each correction
starts with **Correction**. The chain rule stays here as the proposal of the
research.
[ADR 0083](../adr/0083-a-feed-move-refuses-a-loop.md) is Accepted. It answers M1
and M3 with a different rule, and it gives the reasons. No decision is recorded
for M2.

## The Requirement

On 2026-11-04 the operator stated this requirement: a legitimate move always
works. A loop or another problematic link fails, and Studio detects and refuses
it. The Studio request covers the refusal. A publisher on a different tool can
still write a loop, so the node must also be safe.

## Purpose

| # | Change | Evidence |
|---|---|---|
| M1 | Follow a chain of named URLs to its end, and move nothing on a loop | The probe: 4 moves in 12 submissions with no end |
| M2 | Show the operator a mirror that names itself while the source names itself | Four sampled PodcastIndex pairs with one GUID |
| M3 | Decide if a different spelling of one URL can move a record | 13 of 621 sampled tags, and scenario S8 |

## The Defect

Two URLs with one GUID can move a record with no end. Each changed body gives
the next trigger:

1. The record is at A, and the stored value names B. A body from B moves the
   record to B. A move does not write the stored value.
2. The next changed body from B is an update. It stores the value that B
   declares, and that value names A.
3. The next changed body from A moves the record to A.

Each move signs a `FeedUpserted` event and applies the body of that URL. An
unchanged body stops the sequence. ADR 0052 section 1 says that "A move back
needs a new trigger at `F`". It gives no limit, and its guards do not test
this case.

| Scenario | Bodies | Moves on `d807f8b` |
|---|---|---|
| S3 | A names B by tag. B names A by tag. | 4 in 12 submissions |
| S10 | A names B by self link. B names A by self link. | 3 in 8 submissions |
| S11 | A names B by tag. B names A by self link. | 3 in 8 submissions |
| S12 | A names B, B names C and C names A, by tag. | 3 in 12 submissions |

The probe uses the real `/ingest/feed` handler with synthetic bodies and a test
verifier chain.

S8 to S24 are in the capture.

**Observation:** no record in the backup names another record in a loop. Of the
sampled PodcastIndex pairs, none names the other in a loop. The defect has no
known effect today.

**Correction:** the check of the backup cannot find a loop with one GUID,
because the node stores only the values of the source URL. The `feed_upserted`
events of the same backup are better evidence, and they support the conclusion.
They give 1,662 URL changes, and only 1 is after 2026-09-25. No record returned
to an earlier URL after that date. 21 records returned to an earlier URL by
that date. These returns are from the period before or at the deploy of ADR 0051
and ADR 0052.

## M1: The Chain Rule

The proposal changes the mirror case of `move_target` in `src/api.rs`:

1. The source URL names the first URL, as it does today.
2. A body at the expected URL that names no other URL moves the record there.
3. A body at the expected URL that names one other URL makes that URL the
   expected URL. The record does not move yet.
4. A body that names the stored source URL, a visited URL, or two different URLs
   moves nothing. A chain has a limit of 10 hops.
5. A change of the tag at the source URL starts again. A move ends the chain.
6. A move writes the declarations of the body at the new URL.

A refused body gets the usual `source_conflict`. `chain-rule.diff` in the capture
gives the scratch prototype. It is not reviewed code. Each result below comes
from that prototype on a scratch copy.

| Scenario | Without the rule | With the rule |
|---|---|---|
| S3, two URLs that name each other, new bodies | 4 moves | 0 moves, stays at A |
| S3b, the same with unchanged bodies | 1 move | 0 moves, stays at A |
| S10, two self links | 3 moves | 0 moves, stays at A |
| S11, a tag and a self link | 3 moves | 0 moves, stays at A |
| S12, a loop of three URLs | 3 moves | 0 moves, stays at A |
| S15, the tag and the self link at B disagree | 1 move | 0 moves, stays at A |
| S21, a chain ends at the URL of a different record | 1 move to B | 0 moves, `record_conflict` |
| S7, the usual migration | 1 move to B | 1 move to B |
| S14, no tag at the destination | 1 move to B | 1 move to B |
| S13, a chain to C | 2 moves to C | 1 move to C |
| S16, a chain of three hops | Stops at B in this sequence | 1 move to D |
| S17, bodies out of sequence, then a later crawl | Stops at B in this sequence | 1 move to C |
| S18, the source URL is crawled during a chain | Stops at B in this sequence | 1 move to C |
| S20, a chain with unchanged bodies | Stops at B in this sequence | 1 move to C |
| S22, one move and later a second move | 2 moves to C | 2 moves to C |
| S23, a return to an earlier host | 2 moves to A | 2 moves to A |
| S24, a chain ends, and then B names itself | Stops at B in this sequence | Stays at C |

Without the rule, a chain can go on through more crawls. The table shows only the
given sequence.

The 10 test files that use the tag, the self link or the move warning pass with
the prototype, with 96 tests. The full test set did not run with it.

**Correction:** S15 and S21 are losses for the requirement, not only refusals.
In S15 the tag at B names B, and only a stale self link names A. The table
also omits these scenarios, which Stophammer ran with the prototype at `d807f8b`:

| Scenario | Without the rule | With the rule |
|---|---|---|
| R1, A names B, B names X, X is never served, and B then names itself | 1 move to B | 0 moves, stays at A until the tag at A changes |
| R2, the self link at A names B, B names X, A then names itself, and X names itself | 1 move, stays at B | 1 move to X. Rule 5 covers only the tag |
| R3, the operator relocates the record to Y during a chain, and X then names itself | Stays at Y | 1 move from Y to X. `relocate_feed` does not clear the chain |

### What A Decision Must Also Cover

- **The crawler.** The batch walk has three waves, and `gossip` follows two
  levels. **Inference:** a longer chain needs later passes or a crawler change.
  The node can also give the expected URL to the crawler. This was not tested.

  **Correction:** later passes do not help, so a chain of two hops needs a
  crawler change. At wave 2, a music feed gives no follow URL
  (`stophammer-crawler/src/follow.rs:54`), and the crawler never follows a self
  link (`follow.rs:195`). Each pass starts at the stored source URL again.
- **Storage.** The prototype makes its table on use, so a real change needs a
  migration. The chain state can be local to the primary, as the declarations
  are.
- **The guard in ADR 0052.** "An `itunes:new-feed-url` value in a mirror body
  moves nothing" must change for a body at the expected URL. A mirror at any
  other URL still moves nothing.

  **Correction:** the guard "A self link in a mirror body moves nothing" must
  also change. Rule 6 is contrary to the invariant that only an ingest in the
  update case writes `feeds.declared_self_url`. Section 1 also needs an
  amendment, because the source URL does not name the end of a chain.
- **The operator relocation.** **Correction:** the request did not list this.
  A relocation must clear the chain state. R3 shows the result when it does not.
- **A compromised host.** While the record is at A, the host of B can name a
  different URL with the same GUID. ADR 0052 accepts this limit for a source.
  The rule extends it to each URL in a chain.
- **A redirect.** A move by redirect does not change. The probe has no loop that
  mixes a redirect and a tag.
- **Visibility.** A refused loop gives only `source_conflict`. The operator and
  Studio need a way to see the reason. **Correction:** the refused body also
  writes a `feed_copies` row, so the URL is visible. Only the reason is missing.

The research does not propose a cooldown. It would also delay a correct return,
and no incident supports one.

### What The Requirement Cannot Cover

- An old URL that stopped before the node read a move. ADR 0052 keeps this with
  the operator.
- A move with a new GUID. ADR 0052 gives that its own rule.
- A destination that a different record holds. S21 refuses this.
- Two feeds that each name themselves. Neither body states a move. See M2.

### Similar Rules In Other Systems

The note gives the sources. None of these systems reads podcast feeds.

- RFC 9110 says that a client SHOULD detect and intervene in cyclical
  redirections.
- Mastodon refuses a move when the target `also_known_as` does not include the
  old account. It also has a 7-day processing lock and a 30-day cooldown.
- RFC 6596 lets an application ignore an improper canonical designation and
  continue with the other content.
- RFC 2439 suppresses a route that changes too frequently.

## M2: Two Feeds That Name Themselves

When A names A and B names B with one GUID, the first URL that the node admits
holds the record. The body at the second URL gets `source_conflict`, and its tag
is not stored. Scenario S2 shows this.

The research agrees with the result. The tags cannot show which feed is correct.
A choice by date or episode count lets a copy take the record and its payment
routes. ADR 0052 keeps that decision with the source URL.

The gap is that nobody learns about the pair. The first URL can be the wrong
one. PodcastIndex row 4410016 has 393 episodes, and row 6085090 has 31 old
episodes with the same GUID. Each body names itself. Neither URL was tested in
the node.

The proposal records the tag of a mirror as an observation that moves nothing,
and shows the pair in the operator copy queue. The node cannot count these pairs
today, because it does not store the tag of a mirror.

**Correction:** the operator can learn about the pair today. The body at the
second URL writes a `feed_copies` row (`src/api.rs:3067`), and `GET /v1/copies`
lists it. The backup has 84 rows for 59 GUIDs. Only the tag of the mirror is
missing, so M2 is one more value in that row. M2 changes the same queue as
[the request of 2026-11-03](research-lookalike-feeds-and-first-seen-request.md).

Studio can use the same data. The Studio request is
`docs/plans/research-feed-check-request.md` in the `musicindex-studio`
repository. It asks to show
the publisher the index view of a feed. `GET /v1/feeds/{guid}`,
`GET /v1/feeds/{guid}/copies` and `GET /v1/guid-changes` give a part of this
today.

## M3: Spelling

The comparison is exact, as ADR 0051 requires. In scenario S8, the tag names the
same URL with a final slash. The crawler follows it, and the record moves one
time to that spelling. The result is stable and harmless, but the record then
holds the other spelling.

The proposal asks only for a decision. The exact comparison can stay.

## How Other Readers Act

- The published PodcastIndex code stores the tag, and nothing reads the column.
  The API specification and the snapshot have no field for it.
- All seven sampled PodcastIndex pairs where both bodies name themselves keep
  two live rows with no `duplicateOf` value. The live parser is not published,
  so this is limited evidence.
- AntennaPod applies the tag when it differs from the requested URL. The
  research found no destination check there. This was not tested.

## Limits

- The samples select by host and GUID, so they give no rate for all feeds.
- The research did not use the PodcastIndex API.
- The research did not run the prototype on a deployed node.
