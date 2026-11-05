# ADR 0083: A Feed Move Refuses A Loop

## Status
Accepted on 2026-11-04 by the operator, after three security reviews. No reviewer
examined the last commit, `453a52f`. The operator accepted this.
It amends ADR 0052 sections 1 and 2 and one invariant.
Candidate `v0.18.1-rc.1` holds an earlier form of this rule, which "Alternatives
Considered" gives. The next candidate replaces it.

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
- **Left URL:** a URL that the record left by a move or a relocation.
- **Return:** a move to a left URL.
- **Tag:** `itunes:new-feed-url`.
- **Arrival tag:** the tag in the body that moved the record to its current source
  URL.

### 2. The rule

The rule applies to a move by tag or by self link, in the mirror case of ADR 0051.
The node first finds the trigger as in ADR 0052. A tag is examined before a self
link.

- **A move to a URL that is not a left URL goes on.** The current source named that
  URL. The body at that URL refuses nothing.
- **A return needs each of these:**
  1. The trigger is the tag of the current source. A self link cannot start a return.
  2. The body at the left URL has a tag that names that left URL.
  3. The tag of the current source is not the arrival tag.
  4. No cooldown of section 5 is active.

A return with self links only is thus refused. This is a stated exception to the
requirement that a legitimate move always works. The publisher adds the tags, or the
operator relocates the record.

A self link is often stale, because it stays in a body that is copied to a new host.
A tag is a deliberate statement. An old URL can lapse and pass to a different party,
so a return needs the stronger statement at each end. Item 3 refuses a tag that the
body at the current source already carried when the record arrived, because that tag
can be a stale copy.

### 3. A move writes the declarations

A move writes `feeds.declared_self_url` and `feeds.declared_new_feed_url` from the
body at the new URL, under the writer lock of the ingest, as an update does. The same
write stores the `podcast:locked` fact of that body. Thus the next hop needs no
changed body. A mirror body that does not move the record writes neither column.

### 4. The stored history

Two tables hold the history of a record. They are local to the primary, as the
declaration columns are, and no event carries them. They hold no move from before
candidate `v0.18.1-rc.1`, because the event log holds changes from before ADR 0051
that are not moves. Their values do not come from a body that the index serves, so
ADR 0077 requires no refill pass.

- `feed_left_urls` holds the left URLs.
- `feed_move_state` holds the arrival tag, the number of returns and the time of the
  last return. Migration 0061 gives a row to each record that the earlier candidate
  moved, with its stored tag as the arrival tag.

A move adds the old URL, removes the new URL and stores the arrival tag, in the
ingest transaction. An update whose tag differs from the arrival tag clears the
arrival tag, because the source then changed its statement.

An operator relocation adds the old URL, removes the new URL, clears the arrival tag
and sets the number of returns to zero, in the transaction of `relocate_feed`. It
keeps each other left URL, because an operator who moves a record away from a URL
wants it to stay away. The first body from the new source then gives the arrival tag,
because that body can hold a stale copy of a tag.

### 5. A return has a growing cooldown

The first return is free, so that a publisher can correct an error at once. Each later
return waits from the time of the last return:

| Return | Wait |
|---|---|
| 1st | None |
| 2nd | 1 hour |
| 3rd | 2 hours |
| Each later one | Doubles, to a limit of 7 days |

After 30 days with no return, the count starts again. A move to a URL that is not a
left URL never waits and adds nothing to the count.

### 6. A permanent redirect

A permanent redirect to a URL that is not a left URL moves as in ADR 0052. A permanent
redirect to a left URL is a return, and it uses only the cooldown of section 5, because
the source URL states it directly. During a wait, the redirect acts as a `302`: the node
applies the content and keeps the source URL.

### 7. A refusal is visible

A refused body gets the usual `source_conflict`, and it writes the usual `feed_copies`
row of ADR 0058. The response gives this warning, and `tracing::warn!` records the same
values:

```text
move to <URL> refused: the record left that URL (ADR 0083)
```

A redirect in a wait gets an accepted answer with the same warning.

### 8. The crawler does not change

A chain completes with one hop in each pass. The source URL of each pass is the last URL
of the previous pass, so the current follow levels are sufficient.

### 9. The comparison stays exact

Each comparison in this ADR is exact string equality, as ADR 0051 requires. A final slash
can still give one move to that spelling. This result is stable, and this ADR answers
change M3 of the request with no change.

### What this ADR does not decide

Change M2 of the request, the tag of a mirror in the copy queue, has no decision. It
changes the same queue as
[the request of 2026-11-03](../plans/research-lookalike-feeds-and-first-seen-request.md),
so the two get one decision.

A temporary redirect at a named URL stores the named URL with the content of the
redirect target. ADR 0052 calls such a hop delivery only, and the named host can serve any
body directly, so this ADR does not change it.

## Evidence

[The measurement](../reviews/adr-0083-self-link-measurement.md) of 35 real moved pairs gave 1 destination whose tag
names itself and whose self link names the origin. It gave no evidence about a body with no
tag, because the sample selected for that tag. So the choice that a self link refuses no
first move rests on this reason: a wrong refusal strands a correct feed, and a return
cannot follow a wrong first move unless the current source states it by tag.

## Alternatives Considered

### The chain rule of the request
The record waits at the source URL until a chain ends. Rejected:

- A chain of two hops does not finish through the crawler. At wave 2 a music feed gives no
  follow URL, and the crawler never follows a self link. Each pass starts at the stored
  source URL again.
- A dead URL in a chain strands the record until the tag at the source URL changes.
- A stale self link stops a correct move (S15).
- It keeps pending state, which must also clear on a change of the self link and on a
  relocation. The prototype did neither.

### The earlier form of this ADR
It refused a move when the tag of the body named a held URL, and a return when its self
link named one. A security review on 2026-11-04 rejected it. A body with a tag or a self
link that names its own URL could take a record back to a lapsed URL, and a copied body with
its old tag stopped a correct first move.

### Refuse a move when any body names a different URL
This also refuses each correct chain. The operator requirement replaced this earlier
proposal.

### Add no rule
The defect has no known effect. Rejected, because the operator requires a safe node and a
loop can change the payment routes of a record again and again.

## Consequences

- A loop of tags stops. It can make one move for each new URL that it names. The record can
  stop at a URL that a publisher error named. The source URL named it, and the operator can
  relocate the record.
- A publisher who controls the URLs can still move a record back and forth with correct
  tags or redirects. After a count of zero, the cooldown permits up to 8 returns in the first
  week, and then one return each week. It does not stop this, because a source controls its
  own feed (ADR 0056).
- A stale tag or self link at the current source can move a record to a URL that it never
  left. This keeps a correct first move safe from refusal, and it already exists under
  ADR 0052. A move from before candidate `v0.18.1-rc.1` has no left URL row, so its old URL
  counts as a new URL.
- A body behind a temporary redirect writes the declarations, as an update does. So a source
  host that redirects to a different URL lets that URL state the next trigger. This keeps a
  move possible for a feed behind a CDN, and the source host chose the redirect.
- The declarations are written after the ingest transaction. A crash between the two keeps
  the earlier values until the next changed body.
- A stored time in the future, from a clock error, can make a wait longer than 7 days. An
  operator relocation ends the wait.
- **A limit that no rule closes:** the current source can state a return by a real edit of
  its tag, and the old URL can then pass to a different party. To the node, this is a correct
  return.
- A return needs a tag at each end, and a copied tag must first change. The publisher changes
  the body once, or the operator relocates the record.
- Nothing crawls a URL again when its wait ends. A return stays pending until the next
  podping or pass reads the current source URL and that URL.
- A wait can keep a record at a URL with incorrect content for up to 7 days. The source
  can correct its content at once, and an operator relocation ends the wait.
- A chain signs one `FeedUpserted` event for each hop, and it applies each body on the way.
  The record can stay at an intermediate URL between two passes.
- A chain of more than one hop needs more passes. The `gossip` mode reaches the next URL only
  when a podping names the current source URL or the next URL.

## Invariants

These amend ADR 0052:

- Only an ingest in the update or new-feed case of ADR 0051, or a move, writes
  `feeds.declared_self_url` and `feeds.declared_new_feed_url`.
- A self link never starts a return.
- A return never goes to a URL whose body has no tag that names that URL.
- A move to a URL that is not a left URL is never a return, whatever URL the submission
  started from.

## Guards

Each criterion is mechanical. Integration tests in `tests/adr0083_move_refusal_tests.rs`
prove it through `POST /ingest/feed`. This ADR has no visual criterion.

| Scenario | Result |
|---|---|
| S3, S3b, two tags name each other | 1 move, stays at B |
| S10, two self links name each other | 1 move, stays at B |
| S11, a tag at A and a self link at B | 1 move, stays at B |
| S12, a loop of three tags | 2 moves, stays at the third URL |
| A loop of three self links | 2 moves, stays at the third URL |
| S7, S14 and S15 | 1 move to B |
| S13 and S20 | 2 moves to the last URL |
| S16 | 3 moves to the last URL |
| S21, a chain ends at the URL of a different record | 1 move, stays at B |
| S23, a return with a tag at each end | 2 moves to A |
| A stale self link at B, and a body at A with no tag or with a tag | Stays at B |
| A copied tag at B that names A, and a body at A with a tag | Stays at B. It moves after B changes its tag and names A again |
| A return with self links only | Stays at B |
| A second return, 1 hour not passed | Stays. It moves when the hour has passed |
| The count, after 30 days with no return | Starts again |
| A permanent redirect to a left URL during a wait | Applies the content, keeps the source URL, gives the warning |
| The operator relocates, and a tag at the new source then names the old URL with a tag at that URL | Stays at the new source, until that tag changes and names it again |
| A body at an unnamed URL | `source_conflict`, no warning |
| A submission from a left URL that redirects to a new named URL | 1 move to the new URL |
| A return whose body names a URL other than the URL of the return | Stays at the current source |

The ADR 0052 guards "An `itunes:new-feed-url` value in a mirror body moves nothing" and "A self
link in a mirror body moves nothing" stay. A mirror body never gives a trigger.

## Follow-Up

- `docs/tasks/adr-0083-task-005-release-and-record.md` gives the release.
- Studio writes a tag that names its own URL at the destination of a move. The Studio record
  owns that work.
