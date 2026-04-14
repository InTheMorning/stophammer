# ADR 0036: Sign Event Sequence Numbers

## Status
Accepted

## Context

ADR 0004 originally treated `seq` as delivery-ordering metadata outside the
event signature. That allowed signatures to protect event identity and payload
content but not the cursor value used by sync consumers.

The current sync and apply paths use `seq` to advance `node_sync_state`. If a
network attacker could alter an unsigned `seq`, a replica could incorrectly
advance its cursor past legitimate events.

## Decision

Include `seq` in `EventSigningPayload`.

The current signed fields are:

- `event_id`
- `event_type`
- `payload_json`
- `subject_guid`
- `created_at`
- `seq`

The primary assigns `seq` at commit time, signs the payload with that assigned
value, and stores the resulting signature with the event. Community nodes verify
the signature before applying the event and advancing their sync cursor.

## Consequences

- Changing `seq` in transit invalidates the event signature.
- Community nodes can safely use the verified wire `seq` as their primary sync
  cursor.
- Events can no longer be resequenced without re-signing by the event signer.
- This supersedes the `seq` treatment in ADR 0004 and the older community-node
  note in ADR 0009 that described `seq` as excluded from signatures.
