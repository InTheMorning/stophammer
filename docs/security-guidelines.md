# Security Guidelines

This document is advisory. It states no binding rule. It gives security
guidance that applies to more than one part of the code. These ADRs own the
rules:

- [ADR 0006](adr/0006-crawlers-as-untrusted-clients.md) for the crawler trust
  boundary
- [ADR 0051](adr/0051-feed-content-comes-from-its-source-url.md) for the
  source URL of a record
- [ADR 0054](adr/0054-a-fetch-reaches-only-public-feed-hosts.md) for the fetch
  rule
- [ADR 0056](adr/0056-the-public-proof-flow-is-offline.md) for the write routes
- [ADR 0034](adr/0034-adopt-rebuild-first-source-first-v1-music-schema.md)
  for the source-first v1 schema

[operations.md](operations.md) gives the deployment and operator requirements,
and [schema-reference.md](schema-reference.md) gives the current schema.

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

## Documentation Discipline

If a security-relevant limitation is known, document it in the right place.

Use:

- ADRs for architecture and trust-boundary decisions
- `operations.md` for deployment requirements and operator warnings
- the README for short early warnings
- plan docs for temporary implementation sequencing, not permanent security
  policy
