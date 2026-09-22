# ADR 0045: Governance Model And Cross-Repository Contract Ownership

## Status
Proposed

## Date
2026-09-22

## Context
Stophammer and v4vmm share one contract: the MusicIndex HTTP API. v4vmm
reported four defects in that contract. ADR 0042, ADR 0043 and ADR 0044 hold
the decisions that came from those reports.

The two repositories do not share a governance shape. v4vmm ADR 0061 defines a
current-state model. `musicindex-live-publisher` and `splitkit` adopt that ADR
by reference from their own `AGENTS.md`. Stophammer does not.

| Item | v4vmm | Stophammer at `a220f44` |
|---|---|---|
| `AGENTS.md` in version control | Yes | No. `.gitignore` line 12 excludes it |
| `AGENTS.md` purpose | States the present, and holds a dated current-state section | A permanent reference manual with no current-state section |
| Durable and situational rules | Divided. The durable set needs an ADR | Not divided |
| Index of current decisions | `docs/adr/README.md` gives each ADR with its scope and status | `docs/adr/README.md` holds tool notes and no list |
| Archive out of the reading path | `docs/adr/archive/` | None |
| Owner of a rule | `What Binds You` divides agent behavior from code shape | Not stated |

The v4vmm cross-repository table names an owner for five contracts. It names no
owner for the MusicIndex API. That gap let four API defects stay unowned until a
client reported them.

An untracked `AGENTS.md` also breaks one rule that both repositories keep: a
decision and the prose that restates it move in the same commit. A file outside
version control cannot move with a commit.

Stophammer is three repositories, not one. `stophammer`, `stophammer-crawler`
and `stophammer-parser` each have their own GitHub upstream under
`InTheMorning`. The two crate repositories hold no `AGENTS.md` and no `docs`
directory. Every decision that shapes them lives in `stophammer/docs/adr/`.
An agent that works in a crate repository alone reads no rule at all.

## Decision
1. Stophammer adopts v4vmm ADR 0061 by reference. That ADR owns the reading
   path, the durable and situational split, the archive rule, the guard rules
   and the acceptance-criterion rule. This ADR does not restate them.
2. `AGENTS.md` enters version control. `.gitignore` continues to exclude the
   local deployment scripts.
3. `AGENTS.md` gains two sections. `Where The Work Stands` is dated and states
   the current priority. `What Binds You` names `docs/adr/README.md` as the
   index of current decisions, and divides agent behavior from code shape.
4. `docs/adr/README.md` becomes the index of current decisions. Each row gives
   the number, the scope and the status. The `adr-tools` notes stay in that
   file in their own section.
5. Stophammer owns the MusicIndex API contract. ADR 0042, ADR 0043 and
   ADR 0044 own its field rules. A client repository cites those ADRs and does
   not restate the rules. v4vmm adds those rows to its cross-repository table.
6. `docs/adr/archive/` holds a superseded decision. A decision moves there when
   another decision supersedes it, or when a test enforces each rule it states.
   A decision with an unenforced rule stays in the index whatever its status.
7. `stophammer-crawler` and `stophammer-parser` follow the smaller-repository
   shape that v4vmm ADR 0061 defines. Each one gains a short `AGENTS.md` that
   cites this ADR, names `stophammer/docs/adr/` as its decision record, and
   gives its own build and test commands. Neither one starts an ADR set of its
   own while `stophammer` owns its decisions.
8. A change that spans two of the three repositories needs one commit in each.
   The commit message in a crate repository names the owning ADR.

## Alternatives Considered

### Write a governance ADR of our own

Two texts of one model drift apart. Adoption by reference keeps one owner.
Rejected.

### Keep `AGENTS.md` untracked and copy its rules into an ADR

The file that an agent reads first would stay outside review. Rejected.

### Leave the MusicIndex contract without a named owner

The four defects in ADR 0042 to ADR 0044 reached a client before Stophammer saw
them. Rejected.

## Consequences

- A change to `AGENTS.md` enters review with the code that it describes.
- The reading path is bounded. An agent reads `AGENTS.md` and the index.
- Stophammer answers a client statement about the API with an ADR.
- The 44 earlier ADRs need a status check. Some recorded statuses disagree with
  the code. The index records what each file states today.
- The archive move is separate work. This decision moves no file.

## Invariants

- `AGENTS.md` is in version control.
- `AGENTS.md` states no superseded rule and no historical account.
- One repository owns each cross-repository contract. The other repository
  cites that owner.
- A decision and the prose that restates it move in the same commit.
- A crate repository cites its owning ADR. It does not restate the rule.

## Guards

An untracked `AGENTS.md` already broke the same-commit rule. That rule earns a
test.

- A test fails when `AGENTS.md` is absent from the output of `git ls-files`.
- A test compares the ADR files in `docs/adr/` with the rows of the index. It
  fails when a file has no row, or when a row names no file. A file may hold
  more than one row, because the diagnostic table repeats some numbers.
