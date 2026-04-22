# Security Guidelines

This document captures cross-cutting security and identity-resolution rules
that should stay stable even as implementation details change.

It complements, but does not replace:

- [ADR 0006](adr/0006-crawlers-as-untrusted-clients.md) for crawler trust
  boundaries
- [ADR 0018](adr/0018-proof-of-possession-mutations.md) for
  proof-of-possession design
- [operations.md](operations.md) for deployment and operator requirements
- [schema-reference.md](schema-reference.md) for the current schema, and
  [ADR 0034](adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
  for source-first v1 schema boundaries

## Primary Rule

Prefer reversible, evidence-backed decisions over clever irreversible
inference.

In practice:

- false splits are cheaper than false merges
- source facts are stronger than names or heuristics
- derived enrichment may lag if that improves correctness

## Trust Boundaries

### Primary node

The primary is the most sensitive node in the system. It holds the signing key,
authoritative database, and mutation authority.

Rules:

- keep arbitrary outbound fetch behavior out of the primary where possible
- perform privileged verification fetches on the primary only when necessary
- harden any primary-side fetch with SSRF validation, redirect controls, and
  DNS pinning

### Crawlers

Crawlers are separate, untrusted HTTP clients. They are the system's
SSRF-exposed fetch tier.

Rules:

- crawlers may fetch untrusted public URLs
- crawlers should be low-privilege and network-restricted
- crawlers should be able to reach public feed hosts and the primary's ingest
  endpoint, but not arbitrary internal services, metadata endpoints, or
  primary secrets
- `CRAWL_TOKEN` authenticates submission to the primary; it does not make
  crawler fetches safe

### Community nodes

Community nodes are read-only replicas. They do not ingest feeds, do not run
verifiers, and should not diverge from primary-signed state.

## Network Fetching

When stophammer itself performs a fetch that influences authorization or other
security-sensitive decisions, the fetch path must be hardened.

Rules:

- allow only expected schemes, normally `http` and `https`
- reject private, loopback, link-local, and reserved IP destinations
- re-validate each redirect hop
- pin DNS results across the request chain when possible
- bound redirects, timeouts, and response size

Notes:

- plain HTTP remains weaker than HTTPS against DNS poisoning and on-path
  tampering
- transport authenticity and SSRF containment are different problems; both
  matter

## Source Truth and Derived Enrichment

Raw source rows are the immediate truth. Derived identity, classification,
search, and other enrichment layers may take time to converge.

Rules:

- preserve source facts first
- treat enrichment as revisable derived state
- do not imply that all derived fields are ingest-synchronous unless they
  really are
- document when a derived field may be stale between refresh passes

## Identity Resolution

Identity problems should be solved in passes, with the earliest passes making
only the safest assertions.

Rules:

- normalize observed facts before classifying entities
- keep exact technical identity separate from higher-level human grouping
- prefer exact endpoint identity as the conservative anchor
- allow grouping, classification, and artist linkage to remain provisional;
  use explicit confidence states (`provisional`, `high_confidence`, `reviewed`,
  `blocked`) so downstream passes can distinguish settled truth from hypothesis
- weak heuristics must not become identity keys

Examples of weak heuristics:

- display names alone
- split percentages alone
- platform-style labels without supporting context

Examples of stronger signals:

- explicit `fee=true`
- exact same-feed or same-track evidence
- operator review and override

## Proof-of-Possession

The implemented proof level must always be documented honestly.

Rules:

- if only RSS proof is implemented, document the assurance gap explicitly
- machine-readable proof levels should match reality
- future audio proof must use a binary-safe bounded fetch path with an
  explicit maximum byte limit to prevent resource exhaustion on the primary
- audio proof should use format-aware parsing for ID3/FLAC metadata
- fixed-size raw byte scanning is only a fallback heuristic, not the primary
  proof mechanism

## Documentation Discipline

If a security-relevant limitation is known, document it in the right place.

Use:

- ADRs for architecture and trust-boundary decisions
- `operations.md` for deployment requirements and operator warnings
- the README for short early warnings
- plan docs for temporary implementation sequencing, not permanent security
  policy
