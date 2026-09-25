# Feed Identity Conflicts: Scenarios And Tradeoffs

Date: 2026-09-24.

Status: Advisory analysis. Existing ADR requirements remain binding.
No new identity policy is accepted or implemented here.

This analysis extends the [evidence record](feed-identity-conflict-evidence.md).
Its measurements belong to that record. This analysis does not repeat the
production probes or verify the production counts.

The [Podcast Index evidence](feed-identity-podcastindex-evidence.md) provides
external precedents with explicit evidence limits.
The [remediation plan](../plans/feed-trust-remediation-plan.md) gives the
tasks for the decisions in [ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md) to
[ADR 0055](../adr/0055-the-primary-fetches-what-it-signs.md).

This analysis examines protection of payment routes, source history, and
publisher control during recovery from RSS errors. Recommendations below are proposals.
An ADR must own any resulting architecture or contract change.

## Decisions That Differ From This Analysis

The ADRs below took some questions that this analysis left open. In five
places they decide differently from a recommendation here. The ADR is the
owner in each case.

| Topic | This analysis | Decision |
|---|---|---|
| Verification component (section 5) | A worker fetches each feed again after the discovery crawler | ADR 0055: crawlers only nominate URLs. The worker is the only fetcher, so each feed is fetched once |
| A permanent redirect (M1, M2, section 5) | A lasting transfer needs a separate decision | ADR 0052: a `301` or `308` from the source URL moves the record when the target declares the same GUID and is not held |
| Same source, new unused GUID (section 6) | A reversible transition is preferred. The form is open | ADR 0052, as amended on 2026-09-25: the change occurs when the new GUID is the UUIDv5 of the source URL, or after operator approval. A pending change is public. The old record retires. A signed navigation link names the new GUID |
| Repair (section 8, item 2) | Needs a signed retraction of observations | ADR 0051: a forced pass over the source URLs restores content. A mirror observation is a true RSS fact and stays. A retraction event is not decided |
| Immediate containment | A URL-string guard cannot satisfy the security claim | ADR 0051: the guard stops each attacker without the crawl token now. ADR 0055 owns the attacker with the token |

## 1. The Distinctions That Matter

Four questions need separate answers:

1. What did this URL publish at this time?
2. Which source may change this indexed record?
3. Does the new content represent the same release?
4. Who owns the music or has permission to receive payments?

A verified fetch can answer the first question. Continuity evidence can support the
second. Neither establishes the third or fourth in every case.

An artist, publisher, hosting service, and payment recipient can be different
parties. A hosting service can control RSS for many artists. Two legitimate
distributors can publish the same recording under different payment agreements.
RSS control is thus a limited technical authority.

A GUID is a public identity claim. Another publisher can copy it. A matching
title, item GUID, audio file, image, payment address, or publisher link also
does not prove authority. A fresh signing key declared by a conflicting feed
does not establish earlier ownership.

There are cases the available evidence cannot distinguish. A migration copy
and a hostile copy can publish the same RSS at another URL. An accidental
payment change and an authorized payment change can appear the same at the
accepted URL. Automatic handling must expose that limit.

## 2. Qualifications To The Evidence Record

### A missing resource does not authorize a successor

HTTP `404` does not establish permanent removal. HTTP `410` indicates likely
permanent removal. Neither response names an authorized replacement.
[RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html#section-15.5.5)
defines these availability signals.

The security conclusion is an inference: waiting after either response cannot
make an unrelated claimant authoritative. The same applies to DNS failure,
TLS failure, `403`, `429`, and a timeout. Rule H permits eventual takeover of
abandoned records. It is a rejected alternative because failure grants no authority.

### Redirect counts need destination checks

The recorded 1,051 cases include redirects to the current URL or a different
URL. They are not 1,051 verified transfers to the current record.
The other 19 cases are unresolved cases, not 19 established honest migrations.
The measurements do not establish intent or complete coverage of legitimate
moves.

The useful evidence is directional: the accepted source delegates to this
candidate destination. A redirect from the candidate to the accepted source
does not grant the candidate future write authority. A historical observation
also does not establish current delegation.

### A GUID need not match the current URL derivation

The namespace assigns a GUID once and retains it across URL changes.
A GUID derived from an earlier URL can thus differ from a current URL
derivation without error. [The GUID specification](https://podcasting2.org/docs/podcast-namespace/tags/guid)
also gives an RSS-declared GUID precedence over a PodcastIndex assignment.

The incident establishes a changed declaration. The current URL comparison
alone does not establish the tool's derivation input or its reason for changing.
That explanation needs publisher evidence or tool evidence.

### A held URL cannot authorize changes to another held record

Suppose record A uses URL U, and record B uses URL V. U starts declaring GUID B.
Control of U authorizes a statement from U. It does not authorize replacement
of record B. This conflict needs separate handling before any retirement,
alias, merge, or content update.

### The primary owns verification of crawler claims

[ADR 0006](../adr/0006-crawlers-as-untrusted-clients.md) treats crawler
submissions as untrusted. The present node nevertheless depends on submitted
fetch claims for ingest. A crawl token authenticates the submitter. It does
not independently verify the reported URL or redirect.

The primary must verify source authority and content independently of the
messenger's claims. This requirement also covers ordinary same-URL updates.
An authenticated crawler can fabricate a familiar URL, content, and hash.
Comparing its URL strings with stored strings cannot satisfy ADR 0006.
Restricting protection to honest crawlers is not an acceptable remediation scope.

The verification component and its evidence contract need an architectural
decision. ADR 0049 currently keeps ingest fetching in the crawler.
Changing that boundary requires an explicit ADR amendment, including isolation,
fetch limits, and independent parsing of the verified bytes.

### RSS compromise has a defined limit

A rule based only on present RSS control cannot identify the rightful human
after that control is stolen. Previously registered credentials could provide
an additional authority, with registration and recovery costs.
Thus these attacks exceed an RSS-only model, not every possible identity model.

[ADR 0018](../adr/0018-proof-of-possession-mutations.md) currently proves RSS
control. Its audio proof and dual-location relocation proof are not implemented.
Proof at a new claimant URL alone cannot recover a record bound to another URL.

## 3. Complete Structural Cases

Let U be a fetched URL and G its declared GUID. A and B name existing records.
This table describes current bindings. Historical and retired bindings add
the temporal cases in section 4.

| URL lookup | GUID lookup | Meaning | Proposed boundary |
|---|---|---|---|
| U unknown | G unknown | New candidate, undiscovered migration, or clone | Verify source and content before admission. Make no ownership claim |
| U belongs to A | G identifies A | Ordinary source update | Check current source authority and content validity |
| U belongs to A | G unknown | GUID correction or replacement release | Preserve history. Determine continuation separately |
| U unknown | G identifies A | Mirror, migration, copy, or attack | Require authority from A before changing A |
| U belongs to A | G identifies B | Conflict between two held records | Protect both records. No automatic merge or overwrite |
| Any | G missing, malformed, or ambiguous | Insufficient identity evidence | Preserve the previous record and report the defect |

The requested URL, each redirect hop, and the final URL can have different
bindings. Each relevant binding matters. A fetch starting at A can end at a
URL already assigned to B.

The same table applies when both the URL and GUID change. A recorded delegation
may connect the new pair to an old source. Without that evidence, the pair
looks like a new feed. Content similarity cannot supply the missing authority.

## 4. Reasonable Scenarios

Each row states a proposed response, not current behavior. Cases can combine.
For example, a legitimate migration can also encounter stale cache data and a
GUID already used by another feed.

### Publisher and tool errors

| ID | Scenario | Security, integrity, and recovery implications |
|---|---|---|
| P1 | Same URL, new unused GUID, same release | Source authority continues. Preserve both declarations. A permanent alias still needs a continuity decision |
| P2 | Same URL, new unused GUID, different release | The URL is reused. Preserve the old release as history. Do not redirect its track or payment references automatically |
| P3 | GUID A changes to B, then returns to A | A temporary error must be reversible. Avoid irreversible deletion and alias cycles |
| P4 | GUID changes on every fetch | Repeated retirement creates endless identities. Hold the candidate changes and report unstable output |
| P5 | The GUID disappears or becomes malformed | Keep the accepted version. Do not silently replace a declared identity with a newly generated one |
| P6 | A feed gains its first declared GUID after using a fallback | Record the fallback source and the RSS source separately. This is a correction candidate, not proof of a different release |
| P7 | Two GUID elements disagree, or XML namespaces change | Report ambiguity. Parser order or a prefix change must not silently select ownership |
| P8 | UUID spelling changes but its parsed value does not | Separate raw spelling from identity comparison. Do not treat case changes as a transfer |
| P9 | A copied template uses another held GUID | Reject the conflicting write. Let the copy obtain an unused GUID and retry without affecting the original |
| P10 | A platform uses one GUID for many feeds | Isolate each conflict. A blocklist can stop known defaults but cannot establish ownership of arbitrary collisions |
| P11 | Item GUIDs change while the feed GUID stays stable | Track identity breaks independently. Do not automatically merge tracks by title, order, or audio similarity |
| P12 | A feed temporarily omits tracks or value blocks | Preserve the failed candidate and the accepted version. An omission can reflect truncation, feed windowing, or a publishing error |
| P13 | The same URL and GUID start serving a different release | No GUID conflict occurs. Ordinary updates already need an explicit policy for release replacement and mass deletion |
| P14 | A valid payment route changes at the accepted source | This can be authorized, accidental, or hostile. Identity checks cannot distinguish these cases alone |

GUID spelling support is a parser and contract choice. A normalization policy
must preserve raw evidence and cannot silently combine existing conflicting
records. Missing track handling also needs a content policy. Feed identity
rules alone cannot settle whether an RSS document is a complete inventory.

### Moves, mirrors, and ownership changes

| ID | Scenario | Security, integrity, and recovery implications |
|---|---|---|
| M1 | The accepted URL permanently redirects to an unused URL with the same GUID | Strong migration evidence within the fetch trust model. Validate the destination before changing the binding |
| M2 | The accepted URL temporarily redirects to a CDN or proxy | Acceptable delivery can differ from permanent authority. Do not make the temporary destination an indefinite writer |
| M3 | The accepted feed names a new URL, and both URLs remain available | Verify the statement at the accepted source. Validate the destination and define when the old source loses write authority |
| M4 | Two URL forms serve the same feed without a redirect | Content equality supports a mirror hypothesis. It does not prove that either URL may change the other's record |
| M5 | The new URL keeps the GUID while the old URL is unavailable | Preserve the held record. Request independent evidence or operator recovery. Unavailability supplies no transfer authority |
| M6 | A move changes both URL and GUID | Require a connection from the accepted source before treating it as continuity. Otherwise it is a separate candidate |
| M7 | The old URL redirects to an already indexed feed | A may point to B without owning B. Preserve both histories until an explicit consolidation decision |
| M8 | Old and new sources disagree during migration | Select authority through the accepted transfer, not through fetch order. Keep the disagreement visible |
| M9 | A publisher splits one feed into several feeds | A one-to-one alias cannot describe the result. Track mappings and payment references need explicit treatment |
| M10 | Several feeds consolidate into one feed | Do not combine histories, proofs, or track identities from GUID equality alone |
| M11 | The same music has two legitimate distributors or editions | Keep separate source records. Shared audio does not imply shared payment authority or one release identity |
| M12 | An artist changes publisher but keeps the feed | Relationship claims can change independently of source identity. A publisher link grants no mutation authority |
| M13 | A feed, domain, or hosting account changes hands | Present control does not prove continuity of the former owner. Old tokens, aliases, and payment references need review |
| M14 | A former source resumes publishing after an accepted move | Historical authority must not restore current write access. A reverse move needs new evidence from the current authority |

Apple describes a `301` redirect plus a new-feed declaration at the new feed.
Its instructions also preserve episode GUIDs.
[Apple's migration instructions](https://podcasters.apple.com/support/837-change-the-rss-feed-url)
describe client migration behavior.

For Stophammer, a declaration found only at an unrelated new URL cannot prove
delegation from the accepted URL. That is a security inference. A declaration
verified at the accepted source can supply different evidence.

### Hostile claims and misleading evidence

| ID | Scenario | Security, integrity, and recovery implications |
|---|---|---|
| A1 | Another URL copies a held GUID and substitutes payment routes | Deny mutation of the held record. Identical item GUIDs do not reduce the attack |
| A2 | A hostile copy reaches the index before the original | First observation creates an operational binding, not rightful ownership. Provide a dispute path that can correct the first binding |
| A3 | The attacker initially publishes identical content, then changes it | An observed mirror must not silently acquire write authority. Earlier equality cannot authorize later payment changes |
| A4 | Many unrelated URLs claim one victim GUID | Keep valid updates from the accepted source working. Bound conflict storage, fetches, retries, and operator notifications |
| A5 | Another feed links to a victim as its publisher or album | Preserve the claim's source. Discovery and relationship resolution must not grant update authority |
| A6 | An old mirror or delegated host is compromised | Reassess the exact scope and lifetime of delegation. A previously observed URL is not permanently trusted |
| A7 | The accepted domain expires or its hosting account is compromised | An RSS-only rule can accept the attack. Prior credentials, payment change controls, and recovery history can limit consequences |
| A8 | HTTP traffic, DNS, a proxy, or an open redirect supplies false continuity | Evaluate transport and redirect evidence. A redirect is only as strong as the fetch and endpoint controls |
| A9 | A crawler or its token is compromised | Require independently verified content and source authority. Submitted strings and signatures from that crawler cannot establish truth |
| A10 | A candidate copies a proof token or declares a new signing key | Public text is not sufficient. Verify the challenge binding and the previously accepted authority |
| A11 | A deleted or retired GUID is submitted again | Absence from the current table must not erase retirement policy. Require explicit restoration where deletion was intentional |
| A12 | A malicious claimant triggers a global freeze of the victim | Quarantine the candidate claim. Do not let an unauthenticated conflict alone disable the accepted feed or its payments |

`podcast:locked` tells hosting platforms whether import is permitted. Its
optional owner address supports an external verification process.
[The locked specification](https://podcasting2.org/docs/podcast-namespace/tags/locked)
does not make the tag a cryptographic ownership proof.

My security inference is that a copied tag or email address cannot authorize a
transfer. A conflicting claimant must not control the evidence used to settle
its own claim. An operator override needs independently described reasons too.

### Ordering, cache, and storage failures

| ID | Scenario | Security, integrity, and recovery implications |
|---|---|---|
| T1 | A fetch from before a migration arrives afterward | Compare the submission with the current binding. A stale response must not reverse the migration |
| T2 | Two crawlers submit different changes concurrently | Recheck both URL and GUID bindings within the write transaction. A prior reader check is insufficient |
| T3 | A CDN alternates old and new documents | Crawl count is not independent confirmation. Use elapsed time and fresh observations without treating repetition as ownership proof |
| T4 | A `304` reuses cached content after an identity decision | Bind the cache to its source and validation history. Cached bytes do not establish new delegation |
| T5 | An NDJSON replay supplies historical content | Retain its historical origin. Reprocessing must not grant old sources current authority |
| T6 | An unchanged hash skips an identity check | Validate authority before any persistent effect, including URL observations and relationship changes |
| T7 | A process fails between retirement and replacement | One accepted transition needs atomic database effects or an explicit recoverable state |
| T8 | A community node receives only part of a transition | Signed ordering does not itself provide atomic visibility. Define replay, partial progress, and version compatibility |
| T9 | An old alias target is deleted, restored, or reassigned | Avoid alias cycles and changes of meaning. A historical link must not silently resolve to a different release |
| T10 | A redirect or new-feed tag points to an internal address | New verification fetches require SSRF protection, redirect limits, DNS checks, and bounded response sizes |
| T11 | URL comparison removes significant path or query data | Distinct feeds can collapse. Preserve raw URLs and use a specified comparison policy |
| T12 | A platform deployment changes thousands of feeds at once | Bound automatic transitions and expose aggregate impact. Avoid either mass deletion or one manual ticket per crawl |

HTTP and HTTPS, path case, trailing slashes, and query parameters are not
unconditionally interchangeable. A GUID seed normalization rule is not a
general URL identity rule. Domain membership also does not establish authority
between separate accounts on one hosting platform.

## 5. What Evidence Can Authorize

| Evidence | Useful meaning | Limit |
|---|---|---|
| Fresh fetch of the exact accepted source | This source presently serves these bytes | Depends on fetch integrity and present URL control |
| Verified delegation from accepted source to candidate | The accepted source directs delivery or a move | Delivery and permanent authority need different scopes |
| Fresh proof at the accepted source | The requester can place the bound challenge there | Current ADR 0018 proof is RSS-only |
| Proof at both accepted and destination sources | The requester controls both documents now | Does not prove music ownership or historical continuity |
| Previously registered key or recovery credential | Continuity of an earlier authority | Requires registration, revocation, loss, and dispute rules |
| Independent operator evidence | A recorded exception can recover an otherwise blocked case | Operator judgment becomes part of the trust model |
| Same GUID, items, audio, payment address, or metadata | Similarity and possible continuity | Public values can be copied |
| Multiple crawls or a long waiting period | Persistence of the observed state | Persistence supplies no missing authority |
| Old URL unavailable | Need for investigation or archival status | No evidence that a specific claimant is authorized |
| Signed node event | The primary accepted and recorded a decision | Does not prove the RSS publisher's ownership claim |

A proposed transfer record should identify the old binding, destination,
evidence source, observation time, and exact authorized operation. It should
also identify the binding revision against which the evidence was checked.
An earlier proof must not remain useful after that binding changes.

A verified `301` still needs a defined authority effect. Current delivery and
a lasting transfer have different consequences for the former source.
After a lasting transfer, fresh content from the former source must not regain
write authority. This requirement applies even when URL history retains the
former address. The stored URL alone cannot represent both meanings.

Each lasting transfer needs rules for token revocation, pending challenge
invalidation, and authority checks when proofs complete. This includes
legitimate transfers that preserve the stored `feed_url`.

The existing ingest request supplies starting and final URLs. It does not
supply a complete redirect chain, intermediate statuses, or a fetch timestamp.
See [IngestFeedRequest](../../src/ingest.rs) and the
[crawler submission](../../stophammer-crawler/src/crawl.rs).
Those fields alone cannot distinguish every case above.

Independent verification needs an explicit component boundary:

| Model | Benefit | Cost and residual risk |
|---|---|---|
| Primary-controlled isolated verification worker | Keeps network fetching outside the signing process and independent of discovery claims | Requires separate authority, bounded jobs, freshness, replay protection, and verified parsing |
| Node performs verification fetches directly | Provides independent bytes for validation | Changes the existing boundary and adds SSRF exposure to the signing process |

[ADR 0055](../adr/0055-the-primary-fetches-what-it-signs.md) selects a
variant of the first model. The worker is the only fetcher.
Renaming a discovery crawler or giving it another signing key does not provide
independent verification. Its credentials must not authorize verification results.
Accepting authenticated crawler observations alone is a rejected alternative.

A signed crawler statement proves which crawler made the statement. It does
not independently prove that the statement is true. Separating fetch authority
also does not settle who legally owns a release.

## 6. Recovery Choices And Their Costs

### Same source, new unused GUID

| Choice | Honest error recovery | Integrity and operational cost |
|---|---|---|
| Reject and retain accepted state | Requires publisher correction or operator action | Simple, but freezes valid changes |
| Retire and recreate immediately | Handles valid changes quickly | Deletes current child rows and disrupts old references. Temporary errors cause repeated destruction |
| Retire and recreate after confirmation | Reduces damage from brief errors | Delays updates. Repeated bad output still passes confirmation |
| Preserve a reversible identity transition | Supports correction and return to the earlier GUID | Needs transition history and explicit restore behavior |
| Keep a stable internal source identity beside declared GUIDs | Can preserve identity through declaration errors | Changes storage and API mapping. Needs separate release identity and collision handling |

An internal identity is an architecture option. It must not conceal or replace
the RSS declaration in source evidence. Keeping an old identifier can preserve
provenance if the system clearly separates internal identity from declared GUID.
The evidence record's rule E loses provenance when that distinction is absent.

My preferred direction is a reversible transition for an authorized source.
The source can resume useful indexing without an automatic claim that the old
and new releases are identical. The representation of that transition remains
an ADR decision.

### Aliases and payment references

Three relationships have different meanings:

- A URL mirror provides another delivery location.
- A GUID correction continues one release under a corrected declaration.
- A successor release replaces the content at a reused URL.

A generic alias loses those distinctions. A link suitable for navigation can
still be unsafe for automatic payment resolution. A historical track reference
must not silently select the payment route of a different release.

Item GUID overlap can support continuity after source authority is established.
It cannot prove continuity: reused templates can retain item GUIDs across
different releases. Zero overlap also cannot disprove continuity after a tool
rewrites all item GUIDs.

One reasonable default is to preserve a historical identity without automatic
payment forwarding. A separately authorized continuity mapping could restore
old client references. Splits and consolidations need more than a feed alias.

### New source, held GUID

Verified delegation or a scoped recovery decision can authorize a change.
Otherwise, preserving the accepted record prevents an unrelated URL from
replacing its payment routes. The candidate still needs a visible recovery
path and a specific reason for rejection.

This choice can delay legitimate moves after a publisher loses the old host.
Automatically accepting those moves instead permits the same action by an
attacker. Similarity and a timeout cannot remove that tradeoff.

### Keeping two sources that declare one GUID

Rejecting the second source preserves unique GUID lookup but can exclude a
legitimate publisher whose GUID was copied first. The dispute path must allow
correction of that initial binding.

Keeping both source records is another reasonable design. It requires an
internal identifier or source-qualified lookup because the declared GUID is
ambiguous. A GUID-only request must then report ambiguity or apply an explicit
selection decision. It cannot silently select the last submitted payment route.

Minting a replacement GUID internally does not correct the publisher's RSS.
Such an identifier must be identified as internal, with the raw declaration
retained. This option changes the current unique-GUID storage and HTTP contract.

### Payment changes at the accepted source

| Choice | Benefit | Cost |
|---|---|---|
| Apply every valid update from the accepted source | Preserves ordinary RSS operation | Accepts publisher mistakes and compromised-source changes |
| Hold large or unusual payment changes | Limits some accidental damage | Introduces heuristics, delay, false positives, and review work |
| Require a previously registered credential | Adds authority beyond current RSS control | Requires enrollment and key recovery. Existing feeds lack that protection |
| Show a change or conflict and let clients choose | Preserves source facts and client choice | Requires an API contract and client behavior |

A retained old payment route can also become incorrect after an authorized change.
Freezing data is thus not automatically safe. Payment clients need a clear
distinction between current accepted data, historical data, and an unresolved
candidate when policy delays an update.

## 7. Required Integrity Properties For A Proposed Design

These criteria apply existing trust requirements to the proposed remediation.
The transfer and recovery details still require an accepted ADR.

1. Keep raw declarations separate from accepted bindings and continuity decisions.
2. Require authority for each existing record that a transition changes.
3. Prevent unverified claims from changing accepted payments, proofs, aliases, or relationship resolution.
4. Continue valid updates from an accepted source when unrelated candidates make conflicting claims.
5. Preserve enough history to reverse a mistaken identity transition.
6. Record intentional retirement separately from temporary unavailability and identity correction.
7. Prevent historical sources, stale submissions, and replays from restoring revoked authority.
8. Recheck identity bindings inside the transaction that accepts their change.
9. Apply content, identity, search, and signed events as one defined transition.
10. Define community-node recovery and client visibility when a transition is only partly available.
11. Bound conflict retention, candidate fetches, retries, and notifications.
12. Return a specific conflict result with an actionable recovery path.
13. Reconsider a resolved conflict when unchanged RSS would otherwise retain a cached rejection.
14. Verify repairs despite unchanged hashes, and retract incorrect observations through signed operations.
15. Invalidate former authority credentials and pending proofs when a lasting transfer requires it.
16. Authenticate submissions before configurable checks, hash shortcuts, and every persistent effect.
17. Verify RSS bytes and derived fields independently of discovery crawler claims before accepting them.
18. Apply the authority boundary to ingest, proof completion, relocation, and authorized repair.
19. Enforce channel-level proof scope without imposing an unapproved body-GUID equality requirement.
20. Protect pending proofs from unrelated cancellation and from exhaustion of challenge capacity.
21. Recheck proof expiry, pending state, and current authority when issuing a token.
22. Keep independently verified mirror observations separate from permission to replace indexed content.

An HTTP `409` is a reasonable contract option for state conflicts.
The existing ingest envelope can also report a named rejection.
The crawler must distinguish a conflict from a retryable transport error.
The chosen contract needs to preserve that distinction across all crawl modes.

These properties do not require every conflicting body to remain forever.
Bounded evidence can retain hashes, essential differences, timestamps, reasons,
and the accepted version. Retention limits must preserve useful recovery.

## 8. Existing Code That Constrains The Design

### Verified Implementation History

The history was inspected on 2026-09-24. The overwrite path exists in the
initial commit, `4a81de0`, dated 2026-03-09. Ingest finds an existing feed by
URL, but the database updates by GUID. Its verifiers do not check source
authority. Matching track GUIDs allow replacement of payment routes.
[Initial ingest](https://github.com/InTheMorning/stophammer/blob/4a81de016e844c8d1d5250d1d777e079c3a7e345/src/api.rs#L84)
and [initial database write](https://github.com/InTheMorning/stophammer/blob/4a81de016e844c8d1d5250d1d777e079c3a7e345/src/db.rs#L516).

Commit `c812f61`, dated 2026-09-23, changed the URL behavior under ADR 0049.
Earlier ingest replaced the stored URL with the submitted canonical URL.
This change keeps the stored URL while permitting content updates.
It obscures the replacement source but did not introduce the underlying
overwrite path. [Change](https://github.com/InTheMorning/stophammer/commit/c812f614b3a4d870608111a76eadfc61a71a4b17).

The recorded [operator decisions](../plans/v4vmm-publisher-relationship-request.md#operator-decisions)
concern publisher relationships and metadata. They do not authorize unrelated
feeds to replace accepted payment routes. The missing authority check is an
implementation defect, not an unresolved preference about permitting takeover.

The crawler already follows redirects and supplies the final URL for ingest.
The current node can accept content through a redirect while retaining the
stored URL. Correcting the authority check must keep legitimate redirect
handling. Honest GUID correction remains a separate design question.

### Current Code

The current code has useful controls: signed events, transactional mutations,
RSS challenges, constant-time crawl-token comparison, and token revocation after
URL relocation. The proof fetch also limits redirects and pins DNS results.
These controls do not independently authenticate crawler-supplied content.

| Location | Verified code fact | Consequence |
|---|---|---|
| `src/api.rs:1645` | Existing-feed lookup uses `canonical_url` | A conflict check also needs the declared GUID and relevant source bindings |
| `src/api.rs:1836` | An existing GUID keeps its stored `feed_url` | The stored location can differ from the source of accepted content |
| `src/verify.rs:234` | Hash deduplication precedes later default verifiers | A new identity rule cannot depend only on a later verifier |
| `src/verify.rs:293` | Configuration can omit or reorder authentication | Mandatory checks must not depend on configurable order |
| `src/api.rs:1690` | The no-change path can record URL observations | Identity checks must cover this persistent effect too |
| `src/api.rs:1798` | Artist-credit helpers run before the ingest transaction | Reject unauthorized candidates before these helpers can write |
| `src/db.rs:4409` | A URL observation replaces its GUID when the GUID changes | Observation storage is not an immutable history |
| `src/db.rs:4578` | Publisher links resolve through GUIDs and URL observations | Observation changes can affect relationships without changing payment rows |
| `src/db.rs:2651` | Feed deletion removes tracks, routes, proofs, and source claims | Retirement is destructive to the current database projection |
| `src/api.rs:3931` | Proof assertion compares the stored URL with its earlier value | A transfer that preserves this URL needs an additional authority check |
| `src/api.rs:4049` | URL relocation revokes existing proof tokens | Identity correction needs an explicit token policy too |
| `src/api.rs:3671` | A public challenge request cancels pending proofs for that feed and scope | An unrelated requester can interrupt legitimate proof completion |
| `src/proof.rs:410` | TXT extraction searches all XML descendants | Item-level text can satisfy a channel-level proof check |
| `src/ingest.rs:22` | Ingest has no complete redirect evidence or binding revision | Stronger transition checks may require a cross-crate contract change |

The signed event history may retain earlier facts. That does not by itself
restore deleted state or make two separate transitions atomic.
[ADR 0040](../adr/0040-store-track-identity-as-feed-scoped.md) also makes a
feed GUID change a change to every track's composite identity.

[ADR 0049](../adr/0049-publisher-relationships-are-rss-facts.md) owns URL
observations and relationship resolution. A separate record of rejected claims
must not enter its accepted resolution path without a deliberate decision.

An independently verified declaration at a mirror can be a valid RSS fact,
including a copied GUID. Recording that fact must not grant write authority
or imply legal ownership. Existing mirror resolution is an explicit compatibility
requirement. The admission rule must also protect accepted source bindings from
conflicting observations, including conflicts at intermediate redirect URLs.

[ADR 0050](../adr/0050-the-crawler-revalidates-a-feed.md) owns crawler cache
behavior. Cached bytes and current source authority need distinct treatment.

### Initial Security Review Additions

The security review dated 2026-09-24 examined node revision `85093b9`, crawler
revision `1049a41`, and the draft documents. It identified these remediation
gaps through code inspection. No runtime test or production probe verified
these additional sequences.

1. **Fresh fetching can fail to repair changed content.** The
   [node hash check](../../src/verifiers/content_hash.rs) looks up the submitted
   `canonical_url`. An unauthorized submission updates the claimant URL's
   cache in [the ingest handler](../../src/api.rs). The accepted URL can retain
   its earlier hash while its indexed metadata and routes change. Fetching its
   unchanged body can then produce `NO_CHANGE` and leave the incorrect data.

   Approved repair therefore needs independent fresh verification and scoped
   `force_reingest` after mandatory authority validation. The repair test must check restored
   metadata, routes, corrective events, and community state.

2. **Content repair does not retract incorrect URL observations.** The current
   `FeedUrlObserved` [apply handler](../../src/apply.rs) only inserts or replaces
   an association. Refetching the accepted source leaves incorrect observations
   available to [publisher resolution](../../src/db.rs). Repair needs a signed
   retraction operation, with an ADR for the event-contract change. Retiring
   the feed or assigning a false replacement GUID is not an acceptable repair.

   A verified copied GUID declaration is not automatically an incorrect observation.
   Repair must distinguish false attribution from a true declaration without write authority.

3. **Crawler rejection caching can delay resolved conflicts indefinitely.** The
   [crawler response decision](../../stophammer-crawler/src/crawl.rs) skips
   submission after `304` when its cached node answer is `rejected`. Correcting
   authority does not necessarily change RSS bytes or cache validators.
   Resolution needs targeted reconsideration and a fixture with unchanged RSS.

   `--force` resubmits a kept body after `304`. `--no-revalidate` requests fresh
   bytes but does not bypass node hash deduplication. Both flags address those
   current cache layers in a fetching mode. They do not independently verify
   the crawler or authorize a repair. The new verification path must supply
   that assurance.

4. **Proof completion currently checks URL continuity, not a separate authority revision.**
   The [proof handler](../../src/api.rs) compares the stored URL before issuing
   a token. URL relocation through PATCH revokes tokens. A future authority
   transfer that preserves that URL needs explicit token and challenge rules.
   Tests must cover an old token and an in-flight proof after transfer.
   This is a design gap, not a demonstrated failure of an implemented transfer mechanism.

5. **New conflict handling must resist resource exhaustion.** Separate conflict
   records and mirror validation can consume storage, fetch capacity, and
   operator attention. Limits must apply per feed and globally. Changing
   claimant URLs must not defeat them. Tests must verify bounded retention,
   fetches, retries, and notifications while accepted-source updates continue.

The review also requires a decision between current delivery and lasting
authority transfer for `301` handling. Delayed-replay tests alone are
insufficient. A fresh submission from the former source must also fail after
a lasting transfer revokes that source's authority.

### Expanded Audit: Codified Behavior And Proof Paths

The subsequent audit examined ADR 0006, ADR 0018, ADR 0049, ADR 0050, current
handlers, and security tests. Four local probes reproduced these results:

| Probe | Result | Scope |
|---|---|---|
| Omit `crawl_token` from the configured chain | Invalid-token ingest returns `200` and writes a feed | Explicit nondefault configuration. Production configuration was not checked |
| Place `content_hash` before `crawl_token` | Invalid-token unchanged-hash ingest creates a URL observation and emits a signed event | Explicit nondefault order. The default order authenticates first |
| Put the expected TXT string inside an RSS item | `extract_podcast_txt_values` returns that string | Extractor probe. No end-to-end unauthorized token issuance was demonstrated |
| Request a challenge with a different requester nonce | The earlier requester's pending challenge becomes `invalid` | Public HTTP handler probe. RSS control is unnecessary |

The probe file was not kept in the repository, so the probes cannot run
again as written. The Guards of ADR 0051 require tests for the first two
conditions. The [remediation plan](../plans/feed-trust-remediation-plan.md)
lists the other two as defects against ADR 0018.

The first two results conflict with ADR 0006's authentication boundary when
the configured chain permits them. Production exploitability depends on the
actual configuration. Unknown verifier names already fail startup, but valid
names in an unsafe order still construct a chain.

The TXT result violates ADR 0018's explicit channel-level requirement.
Its practical privilege effect depends on whether a host delegates item XML
editing separately from channel editing. Namespace validation alone cannot
establish the required element scope.

Challenge replacement requires a more careful correction than simply removing
it. Commit `73dffce` replaced a per-feed pending limit vulnerable to slot exhaustion.
The current replacement behavior and global pending limit protect against that
earlier problem. Replacement also lets an unrelated requester cancel a valid
pending proof. Invalidated rows remain until expiry, so the pending limit does
not bound all stored rows. A fix must prevent both cancellation and exhaustion.

The existing `v2_challenge_flooding_replaced_for_same_feed` test requires that
replacement behavior. It records the earlier mitigation rather than proving
that legitimate requesters can complete proofs during hostile traffic.

Additional findings come from code inspection, not the four probes:

- Primary startup requires `CRAWL_TOKEN` to exist but permits an empty value.
  An absent variable fails startup. The empty-value case needs a regression test.
- The parser can supply a fallback GUID when RSS declares none. The ingest DTO
  does not retain whether its GUID came from RSS or that fallback. Independent
  parsing must preserve this distinction. See [parser](../../stophammer-parser/src/engine.rs)
  and [ingest DTO](../../src/ingest.rs).
- After a `304`, the crawler can resubmit the kept body's earlier final URL.
  It does not compare that URL with the new response's final URL before reuse.
  Cache revalidation therefore cannot establish current redirect authority.
- Proof verification follows redirects with SSRF controls. It does not check
  whether the destination is already bound to another feed record.
- Authorized feed PATCH writes the supplied URL and revokes tokens. The
  database rejects an exactly duplicated held URL, but the handler does not
  validate destination content or dual-location proof.
- ADR 0018 records dual-location proof as unimplemented. This is a known
  assurance gap, not a newly discovered missing implementation promise.
- ADR 0018 does not require the fetched body GUID to equal the indexed GUID.
  Adding blanket equality could prevent recovery from an honest GUID error.
- Proof completion compares URL strings. An A-to-B-to-A change can escape that
  comparison. Challenge expiry is checked before fetching, not when issuing
  the token. These timing sequences remain unreproduced.

An existing HTTP race test permits either `200` or `409`, depending on timing.
It complements the database-level test but does not deterministically establish
each interleaving. New tests must control the pause before completion and
cover expiry and authority changes. See [proof tests](../../tests/proof_tests.rs).

A redirect to another held record needs explicit collision handling across
ingest, proof, and relocation. The inspected code does not establish that every
such redirect is an unauthorized transfer. Verified delivery, release identity,
and permission to change a second record remain separate questions.

## 9. Evidence And Decisions Still Needed

The following measurements would reduce uncertainty:

- Reclassify the 1,070 potential moves by exact redirect destination and final declared GUID.
- Separate current delegation from historical URL observations.
- Measure conflicting GUID claims without allowing them to change accepted records.
- Measure GUID instability at the same source across independent fetches.
- Check the remaining Doerfelverse feeds and obtain evidence of the publisher tool's change.
- Inventory existing URL observations before treating any as authorization evidence.
- Count clients and stored links that depend on old feed and track GUIDs.
- Measure payment changes that a proposed delay policy would hold.

The policy decisions are:

1. How does the primary obtain independently verified content while preserving the isolated fetch boundary?
2. Which evidence grants temporary delivery access, and which evidence transfers lasting write authority?
3. What reversible behavior follows an unused GUID change at an accepted source?
4. What evidence authorizes continuity aliases and automatic payment resolution?
5. How can a publisher recover after losing the accepted source?
6. What happens to stale payments while an authorized source has unresolved changes?
7. How are mistaken first bindings and intentional retirements corrected?

My recommendation is to establish those boundaries before selecting a storage
mechanism. Safe automation can follow established source authority, preserve
raw changes, and keep recovery reversible. Cases without authority evidence
need a visible conflict path, with the accepted record protected.

Validation: Source inspection, primary specification review, and the four
recorded local audit probes described above. The earlier production counts
were not rerun. No production mutation or implementation change is part of
this document revision.
