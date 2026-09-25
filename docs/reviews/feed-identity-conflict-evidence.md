# Feed Identity Conflict: Evidence

Date: 2026-09-24.

This record is advisory. It retains the original measurements and corrects their
interpretation after the security review. Existing ADRs already govern crawler
trust, proof scope, redirects, and URL observations.
The [analysis](feed-identity-conflict-analysis.md) separates those requirements
from defects and unresolved recovery choices.
[ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md) to
[ADR 0055](../adr/0055-the-primary-fetches-what-it-signs.md) propose the remediation.

## The Question

A feed has two identifiers in the index: its `podcast:guid` and its URL. This
record examines what the node must do when the two disagree with what the index
holds.

There are two conflicts:

1. **Same URL, new GUID.** A URL that the index holds starts to declare a
   different `podcast:guid`.
2. **New URL, held GUID.** A URL that the index does not hold declares a
   `podcast:guid` that the index already holds.

## The Incident

The crawler reported this error on 2026-09-24:

```text
ingest_error: ingest http 500 ... UNIQUE constraint failed: feeds.feed_url:
https://www.doerfelverse.com/artists/elijahlied/all-in-a-day/all-in-a-day.xml
```

| | April 2026 snapshot | Index on 2026-09-24 | Feed on 2026-09-24 |
|---|---|---|---|
| `podcast:guid` | `6fde038d-3585-5d28-93e5-aabe48fd7fa6` | `6fde038d-…` | `25f3e971-453c-507a-97e8-800f38f56415` |
| `lastBuildDate` | 15 Feb 2025 | — | 11 Sep 2026 |
| Item GUIDs | — | 5 tracks | The same 5 item GUIDs |

The channel GUID changed, while the five item GUIDs remained equal.
The feed's `lastBuildDate` does not establish when or why its GUID changed.
Both channel GUIDs are UUIDv5. Neither matches the derivation from the current
feed URL. That comparison does not establish the publisher tool's input or
history. A retained GUID can originate from an earlier URL.

The unchanged item GUIDs support release continuity but do not prove it.

The index holds 51 feeds on `doerfelverse.com`. The other 50 are not examined
for the same change.

## What The Node Does Today

These results come from a probe test on 2026-09-24. The probe ran against the
current code and was then deleted.

| Conflict | Result | Evidence |
|---|---|---|
| Same URL, new unused GUID | HTTP `500`. The feed change fails. The held feed retains its earlier content | The probe, and the incident above |
| New URL, held GUID | HTTP `200`. The node updates the held feed's metadata, supplied tracks, and associated **payment routes**. The stored `feed_url` does not change | The probe |
| New URL, new GUID | A new feed | Normal ingest |

The second row is a defect with a security effect:

- A feed at any URL that the crawler fetches can declare the GUID of another
  feed. The node then replaces the payment routes of that feed.
- The API still shows the original `feed_url`. A reader cannot see that the
  content came from a different URL.
- A later original-source crawl can return `NO_CHANGE` and leave the replacement
  content intact. The URL hash cache does not prove that indexed content matches.
- `src/api.rs` step 7b, from ADR 0049 §1, keeps the stored `feed_url` for a
  known GUID. The new URL goes only to `feed_url_observations`.

The failed feed transaction does not establish that the whole request had no
effects. Artist-credit helpers can write before that transaction.
The [analysis](feed-identity-conflict-analysis.md#8-existing-code-that-constrains-the-design)
records these code-inspected limits and the required repair checks.

### How A URL Reaches The Crawler

A URL that nobody controls on the node side can reach the crawler in these
ways:

- The PodcastIndex snapshot, through the `import` mode. Its documented add
  endpoint accepts submissions with a write-enabled API key.
- A podping notification, through the `gossip` mode.
- A `podcast:remoteItem` in a publisher feed that the index holds. The crawler
  follows these links under ADR 0049 §2.
- An operator list, through the `feed` mode.

[ADR 0006](../adr/0006-crawlers-as-untrusted-clients.md) requires the primary to
treat crawler submissions as untrusted. Current ingest nevertheless depends
on submitted URLs, parsed content, and hashes without independent source verification.
This is an assurance gap, not permission to trust the messenger.
The primary does not fetch RSS during ingest. Its separate proof flow does fetch RSS.

## What The Index Holds

These counts come from three sources. They are the live API, the April 2026
snapshot, and a copy of the production backup of 2026-09-24, 20:01 UTC.

| Measurement | Count |
|---|---|
| Feeds in the index | 10,410 |
| URLs in the index and in the April snapshot | 6,402 |
| Of those, the index GUID differs from the April GUID | 0 |
| April URL not in the index, same GUID at another URL | 1,070 |
| Of those, the April URL redirected to the current URL, or to a different URL | 1,051 |
| Of those, with no redirect | 19 |
| GUIDs declared by more than one URL in the April snapshot | 0 |
| Feeds with more than one observed URL in `feed_url_observations` | 1,619 |

These counts identify 1,070 potential moves. The 1,051 redirects include
destinations other than the current URL. They are not verified legitimate
transfers. The remaining 19 cases are not established honest moves.
The measurements do not establish publisher intent or complete migration history.

The current same-URL insertion path rejects a new unused GUID.
Logs and retained source snapshots can help measure those rejected changes.

## Existing Mechanisms

- **Identity.** `feeds.feed_guid` is the primary key. About 15 tables refer to
  it. Track identity is `(feed_guid, track_guid)` under ADR 0040.
- **`FeedRetired`.** A signed event. It deletes a feed with a cascade, and
  community nodes apply it. Only `DELETE /v1/feeds/{guid}` emits it today.
- **Proof of possession (ADR 0018).** The accepted Phase 1 requires a bound
  channel-level `podcast:txt` challenge at the stored feed URL. A scoped token
  permits `DELETE` or `PATCH`. RSS control does not establish legal ownership
  of music. Current extraction also accepts item-level text, contrary to this
  requirement. Audio proof and dual-location relocation remain unimplemented.
- **`FeedGuidVerifier`.** It rejects a malformed UUID and one known platform
  default GUID.
- **Migration and locking declarations.** No typed ingest field or enforced
  migration rule handles `itunes:new-feed-url`. The parser preserves
  `podcast:locked` in its generic namespace snapshot, but ingest has no typed
  field or enforced locking rule. Preservation does not grant authority.

## Actors And Scenarios

"Honest" means that the publisher makes a mistake or a legitimate change.
"Hostile" means that someone tries to take a record that is not theirs.

| # | Actor | What the index sees | Today |
|---|---|---|---|
| S1 | Honest. A tool changes how it makes the GUID. The release does not change | Same URL, new GUID, same item GUIDs | `500`. The record freezes |
| S2 | Honest. The publisher reuses one URL for a different release | Same URL, new GUID, different item GUIDs | `500` |
| S3 | Honest. A temporary tool error writes a wrong GUID and then corrects it | Same URL: GUID A, then B, then A | `500` on B, then normal |
| S4 | Honest. The feed moves to a new host and keeps its GUID | New URL, held GUID. The old URL redirects, gives `404`, or stays | Content taken from the new URL. `feed_url` stays old |
| S5 | Honest. A template copy keeps the GUID of the original feed | New URL, held GUID, different release | The copy can overwrite the original. Hash deduplication can prevent restoration |
| S6 | Honest. A platform gives one default GUID to many feeds | Many URLs, one GUID | Blocked only if the GUID is in `BAD_GUIDS`. Otherwise as S5 |
| S7 | Honest. A feed had no GUID, and the node used the PodcastIndex GUID. Later the feed adds its own | Same URL, new GUID | As S1 |
| S8 | Hostile. A feed at the attacker's URL declares a held GUID, with the attacker's payment routes | New URL, held GUID | **Takeover of the payment routes**. A normal later crawl need not repair them |
| S9 | Hostile. The attacker takes control of the URL itself: an expired domain, or a compromised host account | Same URL, any change | The attacker can change the routes in place. The attacker can also pass the ADR 0018 proof |
| S10 | Hostile. The accepted URL redirects to the attacker's URL | A reported redirect. Its origin and transport need verification | As S9 within an RSS-only authority model |

S9 and S10 exceed an authority model based only on present RSS control.
Earlier credentials or independent recovery evidence could provide additional
protection. ADR 0018 explicitly records the limits of its RSS-only phase.

S8 is inside what an identity rule can stop. The attacker controls a different
URL, and nothing in the index connects that URL to the GUID.

Item-GUID overlap does not separate S8 from S4, because those values are public.
It also cannot reliably separate S1 from S2. Templates can preserve item GUIDs,
and publishing tools can replace them without changing the release.

## Evidence That Can Connect A URL To A GUID

These signals have different authority limits. The primary must verify source
and content independently of crawler claims before relying on a signal.

| Evidence | Covers | Cost |
|---|---|---|
| Verified redirect from the accepted URL to a candidate | Directional delivery evidence. Lasting transfer needs an explicit rule | Verify the actual fetch and destination conflicts |
| Verified `itunes:new-feed-url` at the accepted source | A source declaration naming a move | Parser support and destination validation |
| Correctly scoped `podcast:txt` proof at the accepted source | Current RSS control | Publisher action and proof defects fixed first |
| Verified accepted source declares a new unused GUID | A source identity change | Separate release continuity from source control |
| The held URL gives `404` or `410` for a period | Unavailability only. No successor is authorized | Investigation or independent recovery evidence |
| An operator decision with recorded independent evidence | A scoped recovery exception | Evidence review and operator authorization |

## Recovery Alternatives Requiring A Decision

These alternatives describe tradeoffs. They do not override existing trust
requirements or approve automatic retirement, transfer, or payment aliases.

### For "same URL, new GUID"

| Rule | S1 | S2 | S3 | Security | Data |
|---|---|---|---|---|---|
| A. Reject with a named reason | Frozen | Frozen | Safe | No new risk | Stale until an operator acts |
| B. Retire the old GUID and ingest the new | Can resume updates | New release needs separate history | Repeated retirement can destroy current state | Requires verified authority. Existing proof permissions do not justify automatic deletion | Child rows and old references need explicit recovery |
| C. Add an alias from the old GUID to the new | Can preserve references after continuity review | Can point old references to another release | Can create incorrect aliases or cycles | Payment forwarding needs separate authorization | Navigation and payment mappings have different risks |
| D. Wait for N crawls or N days | Delayed recovery | Delayed replacement | Can filter brief errors | Repetition provides no missing authority | Stale during the wait. Persistent errors still pass |
| E. Keep the old identifier and accept new content | Can preserve internal identity | Can conceal release replacement | Depends on retained declarations | Requires separate internal and declared identity | Loses provenance if the new declaration is discarded |

### For "new URL, held GUID"

| Rule | S4 | S5 / S6 | S8 | Data |
|---|---|---|---|---|
| F. Today: take the content, keep the old URL | Accepts candidate content without proving a move | Can overwrite the original | **Takeover** | Silent replacement can persist through later crawls |
| G. Require verified delegation or independently authorized recovery | Preserves supported delivery and permits scoped recovery | Conflicting writes are rejected | Unrelated source cannot replace the record | Mirror admission and lasting migration still need explicit rules |
| H. Accept when the held URL stops declaring that GUID | Accepts some moves | Protection ends when the original fails | Permits takeover after unavailability | Rejected security alternative. Failure grants no authority |

## Community Nodes

No GUID correction mechanism is selected. Retirement and recreation would use
separate signed events, but sequence order alone does not provide atomic visibility.
A reversible transition needs a defined replication and recovery contract.
An alias or observation retraction also needs signed semantics that community
nodes can apply. A direct database repair does not supply those semantics.

## Open Measurements

- How many crawls give `UNIQUE constraint failed: feeds.feed_url`. The next
  crawl log gives it.
- How many crawls deliver a held GUID from a new URL, using available logs and retained source records.
- Whether the other 50 Doerfelverse feeds changed their GUIDs.
