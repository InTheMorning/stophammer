# ADR 0001: Record Architecture Decisions

## Status
Accepted

## Context
We need a way to track significant architectural choices made during the development of stophammer. Without a record, the rationale behind decisions becomes lost over time, making it difficult to revisit or challenge them with new information.

## Decision
We will use Architecture Decision Records (ADRs) as described by Michael Nygard. Each ADR is a short document capturing the context, decision, and consequences of one architectural choice. ADRs are stored in `docs/adr/` and numbered sequentially. Once accepted, an ADR is immutable — only its status field may be updated to mark it superseded.

## Consequences
- The history of architectural decisions is permanently preserved in version control.
- New contributors can understand why the system is shaped the way it is.
- Changing a decision requires creating a new ADR, which makes the evolution explicit.
