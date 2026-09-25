# Feed Identity: Podcast Index Evidence

Research date: 2026-09-24.

Status: Advisory. This record informs a future decision. It creates no binding
rule and changes no runtime behavior.

Related records: [local incident](feed-identity-conflict-evidence.md) and
[scenario analysis](feed-identity-conflict-analysis.md).

[ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md) to
[ADR 0055](../adr/0055-the-primary-fetches-what-it-signs.md)
use the local security review. Their verification, authentication, proof,
repair, and resource-limit rules are not Podcast Index guarantees.

## Result

The inspected namespace, API documentation, and maintainer reports provide
different kinds of evidence for four practices:

1. Distinguish the index record from the GUID declared in RSS.
2. Keep earlier URL information and explicit duplicate mappings.
3. Use the namespace's feed-control proof convention when a service verifies control.
4. Give publishers a repair path for conflicts that automation cannot resolve.

The sources do not specify a complete security policy for conflicting GUIDs.
The implications below are recommendations for Stophammer, not Podcast Index
rules.

## 1. A Move Should Keep The GUID

The namespace assigns a podcast GUID once. A host migration should keep that
GUID in the new feed. A declared GUID takes precedence over an assignment
made by Podcast Index. [GUID specification](https://github.com/Podcastindex-org/podcast-namespace/blob/c0ff5caa3729610362ee93f8034454fa41f3c493/docs/tags/guid.md).

On 2023-05-04, Dave Jones confirmed that feed declarations supply the GUID used
by the index. [Maintainer statement in discussion 493](https://github.com/Podcastindex-org/podcast-namespace/discussions/493).

**Implication:** Keep the declared GUID and its source. Permit deliberate
correction from a fallback assignment. A GUID derived from today's URL cannot
validate all earlier GUIDs. These sources do not authorize replacement of
another feed that already declares the same GUID.

## 2. Duplicate GUIDs Occur In Actual Index Data

A database report dated 2023-06-18 lists 4,403 rows with this GUID:
`c9c7bad3-4712-514e-9ebd-d1e208fa1b76`. It includes different show titles and
other repeated GUIDs. These are the reporter's historical SQL results, not new
measurements. [Database issue 30](https://github.com/Podcastindex-org/database/issues/30).

This is the same GUID listed in Stophammer's
[BAD_GUIDS](../../src/verifiers/feed_guid.rs).
The match does not show which publisher tool caused the duplication.

**Implication:** A syntactically correct UUID does not prove exclusive identity. A default-GUID
blocklist handles known defects. A general conflict check is necessary for
other copied or incorrect GUIDs.

## 3. Record Identity And Declared Identity Are Distinct

The API reports an internal numeric `id` alongside `podcastGuid`, `url`, and
`originalUrl`. Its `duplicateOf` field identifies another internal feed ID.
[Feed schema](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/components/properties/feed_podcast.yaml),
[ID definition](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/components/properties/id_feed.yaml),
and [duplicate mapping](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/components/properties/duplicateOf.yaml).

The API describes `originalUrl` as a URL before the current URL. The database
README describes `original_url` as the URL at initial addition. Neither
description promises a complete URL history.
[API field](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/components/properties/originalUrl.yaml)
and [database README](https://github.com/Podcastindex-org/database#sample-export-of-the-newsfeeds-table).

**Implication:** Internal identity and explicit mappings have a practical
precedent. This does not prove that an internal ID survives each merge or that
a duplicate mapping safely forwards payments.

The documented `/podcasts/byguid` response contains one `feed` object. It does
not explain selection among conflicting records.
[Response contract](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/components/responses/podcasts_byguid.yaml).

## 4. Maintainers Use Explicit Repair Operations

On 2024-01-21, Steven Crader described adding the requested feed and marking
earlier records as duplicates. Dave Jones confirmed that he commonly used the
same method. Crader reported more resets and merges in February 2026.
[Crader's procedure](https://github.com/Podcastindex-org/web-ui/issues/307#issuecomment-1902485070),
[Jones's confirmation](https://github.com/Podcastindex-org/web-ui/issues/307#issuecomment-1906050034),
and [2026 repairs](https://github.com/Podcastindex-org/web-ui/issues/307#issuecomment-3911568102).

The discussion also records multiple iVoox URLs returning the same content
without redirects. A later repair encountered a destination URL held by a dead
record. [Mirror report](https://github.com/Podcastindex-org/web-ui/issues/307#issuecomment-1793940751)
and [destination conflict](https://github.com/Podcastindex-org/web-ui/issues/307#issuecomment-2081273710).

**Implication:** Recovery needs checks for existing destination records and
explicit mappings. These examples do not establish a universal automatic merge
rule or the evidence required to authorize each merge.

## 5. Migration Decisions Have Compatibility Costs

On 2024-04-26, Dave Jones described migration barriers involving Apple's feed
URL and disagreement between redirects and `itunes:new-feed-url`. Some apps
used Podcast Index as an Apple API fallback, which constrained URL changes.
[Maintainer explanation](https://github.com/Podcastindex-org/database/issues/40#issuecomment-2079746865).

On 2026-01-23, Jones described a Fountain migration where Apple held the stale
URL. He cautioned that automatic corrections could introduce more errors.
[Later maintainer discussion](https://github.com/Podcastindex-org/podcast-namespace/discussions/737).

**Implication:** External directories provide supporting evidence. They cannot
serve as an unquestionable authority. Record the source, destination, and
disagreements for a proposed transfer. The 2024 behavior is dated operational
evidence, not a verified description of today's complete policy.

## 6. Feed Control Proof Has A Namespace Mechanism

The `podcast:txt` specification defines the `verify` purpose. A service supplies
a string that the publisher places in the feed to demonstrate control.
The `locked` tag instead controls import permission for hosting platforms.
[TXT specification](https://github.com/Podcastindex-org/podcast-namespace/blob/c0ff5caa3729610362ee93f8034454fa41f3c493/docs/tags/txt.md)
and [locked specification](https://github.com/Podcastindex-org/podcast-namespace/blob/c0ff5caa3729610362ee93f8034454fa41f3c493/docs/tags/locked.md).

**Implication:** Stophammer's
[ADR 0018](../adr/0018-proof-of-possession-mutations.md) specifies an established
mechanism. Its current extractor also accepts item-level text, contrary to its
channel-level requirement. The [local analysis](feed-identity-conflict-analysis.md#expanded-audit-codified-behavior-and-proof-paths)
records that reproduced defect and other proof findings.
For a disputed transfer, check control at the accepted source.
Proof at an unrelated claimant URL alone cannot authorize changes elsewhere.
RSS control does not establish ownership of the music.

Discussion 534 proposes easier host-assisted verification. It does not establish
a deployed Podcast Index recovery API. The inspected namespace tag index does
not list `podcast:authorization`.
[Proposal discussion](https://github.com/Podcastindex-org/podcast-namespace/discussions/534)
and [tag index](https://github.com/Podcastindex-org/podcast-namespace/blob/c0ff5caa3729610362ee93f8034454fa41f3c493/docs/1.0.md).

## 7. Unavailable Feeds Have A Recovery Path

The API documentation describes marking a feed dead, usually after 1,000
errors without a successful fetch and parse. It then describes monthly checks.
[Dead-feed definition](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/components/properties/dead.yaml).

A 2020 maintainer explanation used a different threshold and described resetting
counters after recovery. Its threshold is not a current guarantee.
[Historical behavior](https://github.com/Podcastindex-org/docs-api/issues/17#issuecomment-692263078).

**Implication:** Keep availability distinct from authority. Neither source says
an unavailable feed becomes available for another claimant to take over.
Stophammer need not copy the retry threshold.

## 8. Submission History Helps Repair Bad Imports

The documented add endpoint requires a write-enabled API key. In September
2020, Dave Jones described attributing additions to keys and reversing additions
in batches. The latter is a historical implementation statement.
[Add contract](https://github.com/Podcastindex-org/docs-api/blob/caf2697be05746b47200bb8cc1ea93d1ec3d7c3d/api_src/paths/add/byfeedurl.yaml)
and [maintainer statement](https://github.com/Podcastindex-org/docs-api/issues/17#issuecomment-692273883).

**Implication:** Record how a candidate reached the index and why it was
accepted. A repair procedure needs to identify affected records independently
of their declared GUIDs. A submitter credential does not prove submitted RSS
is correct.

## Implications For Stophammer

Stophammer's own [ADR 0006](../adr/0006-crawlers-as-untrusted-clients.md) requires
untrusted crawler submissions. The primary must independently verify source
authority and content before accepting a claim. A crawler token or its signed
statement cannot supply that verification. The Podcast Index sources do not
establish how its deployed infrastructure implements this trust boundary.

The following cases remain useful after that requirement is met:

| Case | Recommended behavior | Limit |
|---|---|---|
| Accepted URL declares a new unused GUID | Preserve the accepted record and report the conflict pending an explicit correction rule | Source control does not settle release continuity or payment aliases |
| Another URL declares a held GUID | Keep the accepted record until delegation or scoped recovery authorizes a transfer | GUID equality alone cannot prove control |
| The proposed GUID or destination URL belongs to another record | Report the conflict and require explicit resolution | Control of one source does not authorize changing another record |

For each case, retain source history and the decision reason. Do not make
a payment alias merely because records share a GUID or similar content.
This is our security recommendation, not a documented Podcast Index guarantee.

A separate internal identity deserves evaluation. The evidence does not require
an immediate storage migration. The immediate remediation must also reject
fabricated content from a crawler claiming to report an accepted URL.
Mandatory authentication and consistent checks across proof, relocation, and
repair come from the local requirements and audit. They are not confirmed
Podcast Index practices.

Mirror discovery remains a separate concern from content write authority.
These sources do not justify removing Stophammer's existing URL-resolution
behavior or assigning lasting authority to every redirect destination.

## Evidence Limits

This review used public specifications, API contracts, issue reports, and
attributed maintainer replies. It did not send test feeds or reproduce
historical counts. The exact live algorithm for competing GUID claims remains
unverified. No cited source establishes a payment-safe alias contract.

The namespace revision is `c0ff5caa3729610362ee93f8034454fa41f3c493`, dated
2026-05-13. The API documentation revision is
`caf2697be05746b47200bb8cc1ea93d1ec3d7c3d`, dated 2026-07-07.
Both repositories were read from public Git checkouts.

The web reader received `403` responses from the main Podcast Index website.
The organization's public repositories and GitHub API supplied the cited
material. Historical observations retain their dates and source attribution.

Document checks: Local links and pinned source paths resolve. The shared STE
checker reports vocabulary findings. No runtime tests apply to this document
change.
