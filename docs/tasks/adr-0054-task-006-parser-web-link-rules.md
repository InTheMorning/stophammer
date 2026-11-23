# ADR 0054 Task 006: The Node Uses The Parser Web Link Rules

Owner: [ADR 0054](../adr/archive/0054-a-fetch-reaches-only-public-feed-hosts.md)
§4 and §4a. musicindex ADR 0086 R-FEED-1 items 2 to 4 put the code in
`stophammer-parser`.

Status: done in commit `2bc81a0`, with stophammer-parser `7bd898a`.

## Goal

- `stophammer-parser` owns `web_url_or_none` and `web_link_for_read`.
- The node pins the parser as a Git dependency at a fixed revision. The node
  keeps no copy of the two functions.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Read rules for a web link | reuse | `stophammer-parser/src/web_link.rs` | The parser owns the rules. `src/model.rs` exports them again |
| Parser tests of the rules | reuse | `stophammer-parser/tests/web_link.rs` | The unit tests of the two functions moved here from the node |

## Acceptance Criteria (mechanical)

1. The node has no copy of the two functions. `src/model.rs` has
   `pub use stophammer_parser::{web_link_for_read, web_url_or_none};`.
2. The node tests of ADR 0054 §4 and §4a pass.
3. `stophammer-parser/tests/web_link.rs` passes.

Visual: none.

## Operator Choices

- Operator choice (2026-11-22): the node rev pin and the parser release tag
  must hold the same parser source (ADR 0063 §1). The tag can be the version
  commit that `scripts/create-release.sh` adds after the pin, because that
  commit changes only `Cargo.toml` and `Cargo.lock`. Before a candidate, a
  person runs `git -C stophammer-parser diff --stat PIN TAG -- src` and the
  output must be empty. No test does this check until an incident occurs.
