# ADR 0006: Crawlers Are Separate, Untrusted HTTP Clients

## Status
Accepted

## Context
The system needs to crawl RSS feeds, parse XML, and submit structured data to the core. Crawling is I/O-bound (many concurrent HTTP requests), XML parsing is CPU-bound per feed, and the crawl surface area is large and evolving. The core node, by contrast, is focused on validation, signing, and storage.

Options considered:
- **Embed a crawler in the Rust binary**: Simpler deployment but couples concerns; a crashed crawler takes down the node
- **Crawlers as a separate Rust crate**: Clean separation but still one language/ecosystem
- **Crawlers as arbitrary HTTP clients**: Maximum flexibility — any language, any deployment model

## Decision
Crawlers are separate processes that communicate with the core via `POST /ingest/feed`. They are authenticated only by a shared `CRAWL_TOKEN` (checked by `CrawlTokenVerifier`). The core treats all crawler submissions as untrusted — every submission passes through the full verifier chain before being accepted. Crawlers can be written in any language (Bun, Python, Go, etc.) and can be deployed independently of the core node.

## Consequences
- The core node remains simple and focused; crawler logic (HTTP retry, XML parsing, rate limiting, platform-specific quirks) lives outside the core.
- Multiple specialized crawlers can coexist: one for podping gossip, one for bulk imports, one for platform-specific APIs.
- The `CRAWL_TOKEN` is a simple shared secret — sufficient for a trusted internal network but not for a fully open submission model.
- If the crawl token is compromised, all submitted data must be reverified; there is currently no per-crawler identity.
- Crawlers are the system's SSRF-exposed fetch tier. They should be deployed as
  low-privilege, network-restricted processes that can reach public feed hosts and the
  primary's ingest endpoint, but not arbitrary internal services, metadata endpoints, or
  primary secrets.
- `CRAWL_TOKEN` authentication does **not** make crawler fetches safe. It only
  authenticates submission to the primary. Fetch hardening and deployment isolation are
  still required on the crawler side.
