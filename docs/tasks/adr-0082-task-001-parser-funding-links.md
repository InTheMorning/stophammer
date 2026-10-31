# ADR 0082 Task 001: The Parser Gives Each Funding Tag As A Link

Owner: [ADR 0082](../adr/0082-a-funding-link-is-a-source-link.md) §1 and §2.
The phase plan is [the funding links plan](../plans/funding-links-phase-plan.md).

Repository: `stophammer-parser`. The operator commits. The commit message
names stophammer ADR 0082.

## Goal

`extract_links` gives one `IngestLink` of the type `funding` for each
`podcast:funding` tag of the channel, of each item and of each live item. The
link has the new optional field `label`, which holds the node value. Each
other link has a null label.

## Context

`extract_links` in `src/engine.rs` reads `<link>` and the Atom links of one
parent node. It decides the type and the extraction path in one `match` on the
namespace, the tag name and the entity type. It takes the URL from `href` for
an Atom link, and from the text for a plain link. A funding tag has its URL in
the attribute `url`, and its display text in the node value.

`IngestLink` in `src/types.rs` has `position`, `link_type`, `url` and
`extraction_path`. The serde derives are behind the feature `serde`. The
crawler enables the feature and sends the type as JSON.

## Files To Inspect

- `src/engine.rs`: `extract_links` and its three callers, `is_podcast_namespace`,
  `child_text`
- `src/types.rs`: `IngestLink`
- `tests/adr0069_link_provenance_tests.rs`, for the form of a test with inline
  XML
- `tests/live_value.rs`, for the XML of a `podcast:liveItem`

## Files Likely To Change

- `src/types.rs`
- `src/engine.rs`
- New: `tests/adr0082_funding_links_tests.rs`

## Constraints

- **The field.** `IngestLink` gets `pub label: Option<String>` after `url`,
  with a doc comment that names ADR 0082 §2. The serde attribute is
  `#[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]`.
  Each existing `IngestLink` literal in `src/engine.rs` sets `label: None`.
- **The match arms.** Two arms in `extract_links`, after the Atom arms:
  `(ns, "funding", "feed")` with the guard `is_podcast_namespace(ns)` gives
  `("funding", "feed.podcast:funding")`, and `(ns, "funding", _)` with the
  same guard gives `("funding", "entity.podcast:funding")`.
- **The URL.** For the type `funding`, the URL is the attribute `url`, with
  the white space at its ends removed. A tag with no attribute or an empty
  value gives no link, as a plain link with no text does today.
- **The label.** For the type `funding`, the label is `child_text` with the
  white space at its ends removed, and `None` for an empty text. For each
  other type, the label is `None`. The parser cuts no text.
- **The position.** The funding links keep their place in the sequence of the
  links of the same parent. No sort.
- **No new rule.** `src/profile.rs`, `src/rule.rs` and `src/phase.rs` change
  not. The link extraction runs outside the phases, as today.
- **Lint.** The crate denies `clippy::pedantic`. Each `#[expect]` has a
  reason.
- **Git.** The agent runs no git write command.

## Do Not Touch

- `src/profile.rs`, `src/rule.rs`, `src/phase.rs`, `src/transform.rs`
- `../stophammer/`, `../stophammer-crawler/`
- The existing fixtures in `tests/fixtures/`

## Implementation Steps

1. Add `label` to `IngestLink`, and set `label: None` in each literal of
   `src/engine.rs`.
2. Add the two match arms and the two branches for the URL and the label in
   `extract_links`.
3. Write `tests/adr0082_funding_links_tests.rs` with one inline feed. The
   channel has a `<link>`, two funding tags with a URL, one funding tag with no
   `url` attribute and one with `url=""`. The second funding tag with a URL has
   a node value of more than 128 characters. The item has one funding tag with
   an empty node value. The live item has one funding tag with a node value.
4. Run the gate.

## Acceptance Criteria

Mechanical, in `tests/adr0082_funding_links_tests.rs`:

- The channel gives three links: the `website` link at position 0, and two
  `funding` links at positions 1 and 2, with the extraction path
  `feed.podcast:funding`. The two tags with no usable URL give no link.
- The label of the second funding link is kept whole, with more than 128
  characters.
- The item gives one `funding` link with the extraction path
  `entity.podcast:funding` and a null label.
- The live item gives one `funding` link with its label.
- The `website` link has a null label.
- Each URL and each label has no white space at its ends.

Mechanical, in the crate:

- `cargo test` passes, with no change to an existing assertion.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check`
  pass.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-parser
cargo build
cargo test
cargo test --test adr0082_funding_links_tests
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the operator, with no workaround, when:

- An existing test asserts the exact field set of `IngestLink` as JSON, and
  the new field changes its result.
- `extract_links` cannot take the URL from an attribute without a change of
  its signature.

## Expected Final Report

1. files changed and created
2. tests run, with the result of the gate
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/docs/adr/0082-a-funding-link-is-a-source-link.md`, §1 and §2
- `~/build/stophammer/docs/tasks/adr-0082-task-001-parser-funding-links.md` (this task)
- `~/build/stophammer/stophammer-parser/AGENTS.md`
- `~/build/stophammer/stophammer-parser/src/engine.rs`, `extract_links` and its callers
- `~/build/stophammer/stophammer-parser/src/types.rs`, `IngestLink`
- `~/build/stophammer/stophammer-parser/tests/adr0069_link_provenance_tests.rs`, for the form

Goal:
- Each `podcast:funding` tag of the channel, an item or a live item gives an `IngestLink` of the type `funding`. The URL comes from the attribute `url`. The node value is the new optional field `label`.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Write each comment and doc comment in Simplified Technical English, with the shared skill `~/.agents/skills/asd-ste100/SKILL.md`.
- The crate denies `clippy::pedantic`.

Do not touch:
- `src/profile.rs`, `src/rule.rs`, `src/phase.rs`, `src/transform.rs`
- the existing fixtures in `tests/fixtures/`
- `../stophammer/`, `../stophammer-crawler/`
- git: run no git write command

Acceptance criteria:
- Each mechanical item of the section "Acceptance Criteria" of this task passes.

Test commands:
- the block of the section "Test Commands" of this task

At the end, report:
1. files changed and created
2. tests run
3. behavior changed
4. deviations from this task
5. unresolved concerns
