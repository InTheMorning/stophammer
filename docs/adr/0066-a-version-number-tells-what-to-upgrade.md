# ADR 0066: A Version Number Tells What To Upgrade

## Status
Accepted on 2026-09-27

## Date
2026-09-27

## Context
Stophammer 0.1.0 is the first release (ADR 0063, ADR 0065). The three
repositories share one tag, and each `Cargo.toml` gives the same version. The
OpenAPI document gives it in `info.version`, and the images carry it in the
tags `0.1.0`, `0.1` and `0`.

No rule says which change raises which part of the number. Three changes affect
three different readers:

| Change | Reader |
|---|---|
| The HTTP API: a route, a field, a parameter | Clients, for example v4vmm and musicindex.org |
| The event protocol: an event type or a payload field | The operators of community nodes |
| Other changes: a fix, crawler behavior, packaging | The operator of the primary |

A new event type needs each community node upgraded before the primary emits
it. [ADR 0044](0044-api-contract-declares-its-fields.md) already makes a
breaking change of `v1` need a new path version.

## Decision

### 1. The version is `MAJOR.MINOR.PATCH`

| Part | Raised when | The reader |
|---|---|---|
| PATCH | Only fixes. No change to the HTTP API or to the event protocol | Upgrades in any order |
| MINOR | A new route, field, parameter, setting, event type or payload field. An old client and an old community node keep working | Can use the new parts. When the event protocol changes, the release notes tell the community nodes to upgrade first |
| MAJOR | A change that breaks an old client or an old community node: a removed or renamed field, route or event type, or a changed meaning | Must change. The HTTP API gets a new path version (ADR 0044) |

A raise of one part sets the parts to its right to 0.

### 2. Before 1.0

While MAJOR is 0, a breaking change raises MINOR. The release notes then name
each break and what a reader must do.

`1.0.0` is a promise: from that release, the `v1` API and the event protocol
change only in compatible ways within the same MAJOR.

### 3. One version for the three repositories

`stophammer`, `stophammer-crawler` and `stophammer-parser` release together
with one version (ADR 0063 §1). A change in one repository raises the shared
version.

### 4. The check before a candidate

Before the operator gives the tag of a candidate, the operator or an agent
checks each change since the last release:

1. A breaking change: raise MAJOR, or MINOR before 1.0, and name the break in
   the notes.
2. Else a new route, field, parameter, setting, event type or payload field:
   raise MINOR.
3. Else: raise PATCH.

Each `Cargo.toml` gives the new version in the commit that the candidate
tags. The release notes state whether community nodes must upgrade first.

## Alternatives Considered

### Calendar versions
A version such as `2026.10.1` is simple. It says nothing about compatibility,
so each reader must read the notes to know the order of the upgrade. The image
tags `0.1` and `0` would lose their meaning. Rejected.

### No rule
Each release picks a number. A community operator cannot trust `0.1` to give
only compatible changes. Rejected.

## Consequences

- The next release is 0.2.0. Since 0.1.0, ADR 0064 added a route, fields and
  payload fields. No event type is new, so community nodes can upgrade after
  the primary. A community node stores the relay link only after its upgrade.
- An operator who follows the image tag `0.2` gets each PATCH of 0.2, and no
  MINOR by surprise.
- A client reads `info.version` to know which fields can exist.

## Invariants

- The tag, each `Cargo.toml` and `info.version` give the same version.
- A PATCH release changes no route, field, parameter or event.

## Guards

No release has given a wrong number yet, so no test enforces the rules. The
manual check of section 4 replaces a test. A check in the release workflow,
that the tag and each `Cargo.toml` agree, can come when a release breaks the
first invariant.
