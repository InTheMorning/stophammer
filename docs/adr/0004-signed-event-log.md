# ADR 0004: Nostr-Style ed25519 Signed Event Log

## Status
Superseded by [ADR 0036](0036-sign-event-sequence-numbers.md) for sequence-number signing; otherwise accepted

## Context
Stophammer nodes sync state by exchanging events. Community nodes receive events from the primary and must be able to verify they were not tampered with in transit. The system also needs to be auditable — operators should be able to verify the provenance of any piece of data.

Design constraints:
- Events must be verifiable without trusting the transport layer
- The signing scheme must be simple enough to implement in any language (for future crawler/client integration)
- Sequence numbers are assigned by the primary at commit time and are not part of the content integrity guarantee — they are delivery-ordering metadata only

Alternatives considered:
- **HMAC with shared secret**: Simpler but requires all nodes to share a secret; does not support public auditability
- **RSA signatures**: Larger keys, slower, more complex key management
- **Nostr event format (NIP-01)**: Close to our needs but couples us to the Nostr protocol and key format conventions

## Decision
Each node generates an ed25519 keypair on first boot, persisted as 32 raw bytes at `KEY_PATH` (default: `signing.key`) with 0o600 permissions. Every event is signed over `sha256(canonical_json(EventSigningPayload))`. The signing payload explicitly excludes `seq` — sequence is a delivery-ordering field assigned by the database, not part of content integrity. Signatures are stored in the `events` table alongside the event for offline verification.

The `EventSigningPayload` fields are:
- `event_id` (UUID v4)
- `event_type` (snake_case string)
- `payload_json` (pre-serialized payload — avoids re-encoding ambiguity)
- `subject_guid` (the feed/track/artist GUID this event is about)
- `created_at` (unix seconds)

## Consequences
- Any node or external auditor can verify the signature on any event using only the signer's public key hex.
- The `payload_json` pre-serialization ensures the bytes that were signed are exactly what is stored — no round-trip deserialization ambiguity.
- Excluding `seq` from the signature means events can be re-ordered during sync reconciliation without invalidating signatures.
- Key rotation requires a new ADR; there is currently no mechanism for key rotation or revocation.
