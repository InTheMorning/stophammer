# Research: The History Of The ADR 0018 Proof Flow

Date: 2026-09-25.

Status: advisory. This record gives evidence for a decision about the proof
flow of [ADR 0018](../adr/0018-proof-of-possession-mutations.md). It states no
rule.

## The Question

The review of 2026-09-24 found four defects in the proof flow. Two questions
decide the next change:

1. How did the code reach its current behavior, and which parts were
   decisions?
2. Which design for challenge admission prevents both known attacks?

## History

| Date | Commit or record | Change |
|---|---|---|
| 2026-03-14 | `35ef735` | ADR 0018 and Phase 1. The extractor uses `quick_xml`. It accepts a `podcast:txt` at any depth inside `<channel>`, so a `podcast:txt` inside an `<item>` also passes. A feed can hold at most 20 pending challenges. The 21st answers `429`. |
| 2026-03-14 | `37f9d40` | ADR 0018 records the phase 3 check: the node compares the stored URL with the URL of the fetch before it issues a token. |
| 2026-03-15 | `0995189` | The extractor becomes `roxmltree` with `descendants()`. It accepts a `podcast:txt` at any depth in the whole document. ADR 0018 gets its status table, which says "at channel level". |
| 2026-03-18 | `a9e821e` | A global limit of 5,000 pending challenges. |
| 2026-03-18 | `73dffce` | The limit of 20 for each feed is removed. A new challenge sets each pending challenge of the same feed and scope to `invalid`. The code comment gives the reason: a caller can replace stale or attacker-created challenges, not be blocked by them. ADR 0018 does not change. The commit message gives no reason. |
| 2026-03-22 | `0c98d36` | The last change to ADR 0018: a note about a binary-safe fetch for Phase 2. |
| 2026-03-25 to 2026-04-17 | `docs/security/` | An audit series. The first availability report records finding AVAIL-01, the exhaustion of the challenge table, and its fix, the limit of 20. The second report marks AVAIL-01 closed by the replacement. It does not consider cancellation. The second crypto report describes the `quick_xml` extractor that `0995189` removed. `findings-validation.md` names these reports as the current sources of truth. |
| 2026-09-24 | the conflict analysis | Probes show that a `podcast:txt` in an item passes, and that a second requester cancels the challenge of the first. |

### What was a decision and what was not

- The limit of 20 was a fix for finding AVAIL-01. No ADR records it.
- The replacement rule is a code change. No ADR records it. `docs/API.md` and
  `docs/operations.md` describe it.
- The acceptance of a `podcast:txt` in an item was never a decision. ADR 0018
  says "at channel level". Neither extractor did that, and no test covers it.
- The expiry check before the fetch, and the URL check at issue, were decisions
  of `37f9d40`. They did not consider a change from URL A to B and back.

## Use In Production

The backup of 2026-09-24 20:01 UTC holds:

| Table or event | Rows |
|---|---:|
| `proof_challenges` | 0 |
| `proof_tokens` | 0 |
| `feed_retired` events | 0 |

The pruner deletes expired challenges and tokens, so a count of 0 does not
prove that no one used the flow. No feed was ever retired, by the operator or
by a publisher. No evidence shows a publisher who depends on the flow.

## The Two Attacks

Both attacks need no credential. `POST /v1/proofs/challenge` is public.

| Admission design | Attack | Cost to the attacker |
|---|---|---|
| A limit for each feed (2026-03-14 to 03-18) | Fill the 20 slots of one feed. The publisher gets `429` for 24 hours | 20 requests each day for each feed |
| Replacement (since 2026-03-18) | Send one challenge for the feed while the publisher edits the RSS. The challenge of the publisher becomes `invalid` | One request for each attempt of the publisher |
| Replacement with the global limit | Send one challenge for each of 5,000 feeds. Each new challenge on the node then answers the global limit for 24 hours | 5,000 requests each day. The index holds more than 10,000 feeds |

Each stored design gives one of these attacks. The table of pending
challenges is the shared cause: a stranger can fill it or clear it.

## Options

| Option | Stops cancellation | Stops exhaustion | Cost |
|---|---|---|---|
| 1. Keep replacement | No | Only for each feed. The global limit stays open | None |
| 2. The limit for each feed, with a 1-hour expiry | Yes | No. An attacker blocks one feed for 1 hour at a time | Small |
| 3. Replace only the challenges of the same requester | Yes | No. An attacker fills the global limit | Small. A requester is only a nonce |
| 4. Signed challenges with no stored pending state | Yes | Yes | An amendment to ADR 0018. A used-challenge table written only at assertion. A revision number on the feed for the check at issue, which needs a migration |
| 5. A publisher key declared in the RSS | Yes | Yes. There is no challenge | A new ADR. A publisher must manage a key. Gives a continuing identity for recovery |
| 6. Remove the public flow until a design is chosen | Yes | Yes | Publishers lose a flow that no evidence shows they use. The operator path stays |

### Notes on option 4

The node signs the feed GUID, the scope, the hash of the requester nonce, the
expiry and a revision number of the feed. The assertion checks the signature,
the expiry and the revision, and writes the challenge ID to a used table. That
table grows only with assertions, and each assertion needs an RSS fetch.

A change from URL A to B and back changes the revision number. Thus the fix
for the fourth defect also needs the revision. ADR 0052 also needs a revision
number when a feed moves.

### Notes on option 5

The publisher puts a public key in a `podcast:txt` of the channel. Each
request of the publisher carries a signature with that key. The node fetches
the RSS once to learn the key. No challenge exists, so neither attack exists.
This is also the "previously registered credential" that the conflict
analysis names for recovery after an RSS compromise. It is a larger design.

## The Three Defects That Need No Decision

These fixes enforce ADR 0018 as written:

1. Read only a `podcast:txt` that is a direct child of `<channel>`.
2. Check the expiry again in the transaction that issues the token.
3. Relocation sets each pending challenge of the feed to `invalid` in its
   transaction. The resolve at issue requires `pending`, so a proof in flight
   then fails.

Fix 3 closes the change from A to B and back only while challenges are stored.
Option 4 replaces it with the revision number.

## Documents That Are Now Incorrect

- `docs/security/crypto-blackbox-report-v2.md` describes the removed
  `quick_xml` extractor and calls it channel-only.
- `docs/security/availability-blackbox-report-v2.md` marks AVAIL-01 closed and
  does not name cancellation or the global limit.
- `docs/security/findings-validation.md` names both as current.
- ADR 0018 says "at channel level" and does not record the replacement rule.

The change that fixes the defects also corrects or archives these statements.

## Recommendation

- Now: the three fixes that need no decision, and the correction of the
  documents.
- For admission: option 4 if the proof flow is to stay public, because it is
  the only stored design that closes both attacks. Option 6 is the smaller
  step if no publisher needs the flow soon. Option 5 is worth a separate ADR if
  a continuing publisher identity becomes a goal.

## Not Verified

- If any publisher used the flow before the pruner removed the rows.
- The time a publisher needs between a challenge and its assertion.
