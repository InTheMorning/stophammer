# ADR 0083: A Feed Move Refuses A Loop

## Status
Proposed on 2026-11-04. It waits for the measurement in "Open Question". When
accepted, it amends ADR 0052 sections 1 and 2 and one invariant.

## Date
2026-11-04

## Context
ADR 0052 lets the body at the source URL name a new URL by `itunes:new-feed-url`
or by its self link. When a body arrives from that URL, the record moves.
[The research request](../plans/research-feed-move-confirmation-request.md)
shows that two URLs with one GUID can move a record with no end. Each changed
body gives the next trigger. ADR 0052 gives no limit.

On 2026-11-04 the operator stated this requirement: a legitimate move always
works, and a loop fails. Studio refuses a loop, but a publisher on a different
tool can still write one, so the node must also be safe.

The defect has no known effect. The `feed_upserted` events of the backup of
2026-10-31 give 1,662 URL changes, and only 1 is after 2026-09-25. No record
returned to an earlier URL after that date. Moves by tag are rare: 6 of 10,649
feeds have a stored tag.

The request proposes a chain rule. The record does not move until a body names
no other URL. This ADR does not take that rule. "Alternatives Considered" gives
the reasons.

## Decision

### 1. Terms

- **Own URL:** the `source_url` or the `canonical_url` of the submission.
- **Left URL:** a URL that the record left by a move.
- **Held URL:** the stored source URL, or a left URL. An own URL is never a held
  URL in this test.
- **Return:** a move to a left URL.

### 2. The refusal rule

The rule applies only to a move by tag or by self link, in the mirror case of
ADR 0051. The node first finds the trigger as in ADR 0052. Then it examines the
body in this sequence, and the first match decides:

1. The tag of the body names an own URL: the record moves. A stale self link does
   not stop this move.
2. The tag of the body names a held URL: nothing moves.
3. The move is a return, and the self link of the body names a held URL: nothing
   moves.
4. In each other condition, the record moves as in ADR 0052.

A self link does not refuse a first move to a URL. "Open Question" gives the
reason. A body that names a new URL does not stop the move. The record moves one
hop, and the new source URL then starts the next hop.

### 3. A move writes the declarations

A move writes `feeds.declared_self_url` and `feeds.declared_new_feed_url` from
the body at the new URL, under the writer lock of the ingest, as an update does.
The same write stores the `podcast:locked` fact of that body. Thus the next hop
needs no changed body. Mirror bodies that do not move the record still write neither
column.

### 4. The left URLs

A new table holds the left URLs for each GUID. It is local to the primary, as the
declaration columns are, and no event carries it. The table starts empty, because
the event log holds changes from before ADR 0051 that are not moves. Its values do
not come from a feed body, so ADR 0077 requires no refill pass.

- A move adds the old URL and removes the new URL.
- A permanent redirect also adds the old URL.
- An operator relocation deletes each row of the GUID, in the transaction of
  `relocate_feed`. The operator decision starts a new history.

### 5. A permanent redirect does not change

A move by a permanent redirect uses no test of section 2. The source URL sends the
redirect, so the old URL gives no body that can start a return.

### 6. A refusal is visible

A refused body gets the usual `source_conflict`, and it writes the usual
`feed_copies` row of ADR 0058. The response also gives a warning that names the
held URL and this ADR, and `tracing::warn!` records the same values.

### 7. The crawler does not change

A chain completes with one hop in each pass. The source URL of each pass is the
last URL of the previous pass, so the current follow levels are sufficient.

### 8. The comparison stays exact

Each comparison in this ADR is exact string equality, as ADR 0051 requires. A
final slash can still give one move to that spelling. This result is stable, and
this ADR answers change M3 of the request with no change.

### What this ADR does not decide

Change M2 of the request, the tag of a mirror in the copy queue, has no decision.
It changes the same queue as
[the request of 2026-11-03](../plans/research-lookalike-feeds-and-first-seen-request.md),
so the two get one decision.

## Open Question

**Does a self link at a new URL refuse a first move?** The body names no own URL
by tag, and its self link names the stored source URL. A true loop and a feed
copied to a new host with a stale self link give the same body.

This ADR proposes that the first move goes on, for these reasons:

- A wrong refusal strands a correct feed at the old URL. This is contrary to the
  requirement, and the old URL can stop at any time.
- A wrong move is limited to one move, to a URL that the source URL named. Rule 3
  refuses the return.

The research measures the 35 pairs in the class "one names other, other self" of
its capture of 2026-11-04. That capture has no self links today.

- If no measured body has this form, or the new URL is correct for each body of
  this form: section 2 stays, and the operator can accept this ADR.
- If a measured body of this form shows that the old URL is correct: rule 3 applies
  to each move, and this ADR changes before the operator accepts it.

Zero of 35 proves nothing about all feeds, so a count of zero does not change
section 2.

## Results On A Scratch Copy

A scratch patch of this rule ran on commit `d807f8b` with the research probe and
with more scenarios. It is not reviewed code. Each scenario uses synthetic bodies
and a test verifier chain. Each loop scenario sends new bodies repeatedly, and the
record stays at the given URL.

| Scenario | ADR 0052 today | Chain rule | This ADR |
|---|---|---|---|
| S3, two tags name each other | 4 moves | 0 moves, at A | 0 moves, at A |
| S10, two self links name each other | 3 moves | 0 moves, at A | 1 move, stays at B |
| S11, a tag at A and a self link at B | 3 moves | 0 moves, at A | 1 move, stays at B |
| S12, a loop of three tags | 3 moves | 0 moves, at A | 1 move, stays at B |
| A loop of three self links | Repeats, ends at A | Not run | 2 moves, stays at the third URL |
| S15, the tag at B names B, the self link names A | 1 move to B | 0 moves, at A | 1 move to B |
| S7, the usual migration | 1 move to B | 1 move to B | 1 move to B |
| S13, a chain to C | 2 moves to C | 1 move to C | 2 moves to C |
| S16, a chain of three hops | Stops at B | 1 move to D | 3 moves to D |
| S20, a chain with unchanged bodies | Stops at B | 1 move to C | 2 moves to C |
| S21, a chain ends at the URL of a different record | 1 move to B | 0 moves, at A | 1 move, stays at B |
| S23, a return to an earlier host | 2 moves to A | 2 moves to A | 2 moves to A |
| A names B, B names X, X is never served, B then names itself | 1 move to B | 0 moves, at A | 1 move, stays at B |
| The operator relocates to Y during a chain, X then names itself | Stays at Y | 1 move to X | Stays at Y |

With the patch, the full test set gives 1,190 passes and 1 failure. The failure is
`agents_md_is_tracked_by_git`, because the scratch copy is not a Git repository.

## Alternatives Considered

### The chain rule of the request
The record waits at the source URL until a chain ends. Rejected:

- A chain of two hops does not finish through the crawler. At wave 2 a music feed
  gives no follow URL, and the crawler never follows a self link. Each pass starts
  at the stored source URL again.
- A dead URL in a chain strands the record until the tag at the source URL changes.
- A stale self link stops a correct move (S15).
- It keeps pending state, which must also clear on a change of the self link and on
  a relocation. The prototype did neither.

The chain rule makes fewer moves. Each move signs one `FeedUpserted` event, and
moves are rare, so this benefit is small.

### Refuse a move when any body names a different URL
This also refuses each correct chain. The operator requirement replaced this
earlier proposal.

### A cooldown between moves
It also delays a correct return, and no incident supports it. Rejected.

### Add no rule
The defect has no known effect. Rejected, because the operator requires a safe node
and a loop can change the payment routes of a record again and again.

## Consequences

- A loop stops after at most one move for each URL that it names. The record can
  stop at a URL that a publisher error named. The source URL named it, and the
  operator can relocate the record.
- A chain signs one `FeedUpserted` event for each hop, and it applies each body on
  the way. The record can stay at an intermediate URL between two passes.
- A chain of more than one hop needs more passes. The `gossip` mode reaches the next
  URL only when a podping names the current source URL or the next URL.
- A correct return stays refused while the old URL still names the current URL. The
  publisher corrects the old URL, or the operator relocates the record.
- A compromised host of a named URL can move the record onward. ADR 0052 accepts
  this limit for a source, and this ADR does not change who holds the source URL.

## Invariants

These amend ADR 0052 when this ADR is accepted:

- Only an ingest in the update case of ADR 0051, or a move, writes
  `feeds.declared_self_url` and `feeds.declared_new_feed_url`.
- A move never goes to a URL when the tag of its body names a held URL.
- A return is refused when the self link of its body names a held URL, unless its
  tag names an own URL.

## Guards

Each criterion is mechanical, and integration tests in `tests/` prove it through
`POST /ingest/feed`. This ADR has no visual criterion.

- S3, S10, S11 and S12 stop with the result in the table above after repeated new
  bodies.
- S7, S13, S14, S15, S16, S20 and S23 complete with the result in the table above.
- A move writes the two declaration columns from the body at the new URL.
- A mirror body that does not move still writes neither column.
- A relocation deletes the left URLs of the record.
- A permanent redirect moves the record when the body at the new URL names the old
  URL.
- A refused body gets `source_conflict`, a `feed_copies` row and the warning.

The ADR 0052 guards "An `itunes:new-feed-url` value in a mirror body moves nothing"
and "A self link in a mirror body moves nothing" stay. A mirror body gives only a
refusal under this rule, and it never gives a trigger.

## Follow-Up

- The research sends the measurement. The operator then decides the open question.
- `docs/tasks/adr-0083-task-001-measurement-and-acceptance.md` to
  `adr-0083-task-005-release-and-record.md` give the decision, the build and the
  release. The build starts after acceptance.
- Studio refuses a loop before it publishes. The Studio request owns that work.
