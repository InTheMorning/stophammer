# ADR 0005: Pluggable Verifier Chain for Feed Ingest

## Status
Accepted

## Context
The ingest endpoint must validate submitted feeds against a set of rules before writing. These rules need to evolve as new data quality issues are discovered (e.g., Buzzsprout podping spam, platform-specific bad GUIDs). Adding new validation logic should not require touching the core ingest handler.

Additionally, some validation failures are informational (the feed should still be accepted, but the anomaly should be recorded) while others are hard rejections.

## Decision
We implement a `Verifier` trait with a single method `verify(&IngestContext) -> VerifyResult`. `VerifyResult` has three variants:

- `Pass` — no action
- `Warn(String)` — accepted, warning stored with the event for audit
- `Fail(String)` — rejected, reason returned to the crawler

A `VerifierChain` holds a `Vec<Box<dyn Verifier>>` and runs them in order, collecting warnings and returning on first `Fail`. New verifiers are registered in `main.rs` by pushing to the chain — no changes to core ingest logic required.

A special sentinel `Fail("NO_CHANGE")` is used by `ContentHashVerifier` to signal a no-op (identical content hash) rather than a true rejection. The ingest handler checks for this sentinel before returning an error response.

Built-in verifiers (in order):
1. `CrawlTokenVerifier` — authenticates the crawler
2. `MediumMusicVerifier` — enforces `podcast:medium = music`
3. `FeedGuidVerifier` — rejects known bad/placeholder GUIDs and malformed UUIDs
4. `PaymentRouteSumVerifier` — rejects tracks where payment splits don't sum to 100
5. `ContentHashVerifier` — no-op sentinel for unchanged feeds
6. `EnclosureTypeVerifier` — warns on video enclosure types

## Consequences
- New data quality rules (e.g., platform-specific blocklists, minimum track duration) are added as new `Verifier` impls with no changes to the handler.
- Warnings are stored permanently with each event, creating an audit trail of accepted-but-suspicious ingestions.
- The sentinel pattern for NO_CHANGE couples the ingest handler to a specific string constant — this is a known tradeoff accepted for simplicity.
- The `Verifier` trait requires `Send + Sync` so the chain can be shared across async tasks.
