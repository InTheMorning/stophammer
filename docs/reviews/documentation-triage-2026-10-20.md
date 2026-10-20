# Documentation Triage

Date: 2026-10-20. Base commit: `af8c545`.

This record is advisory. It states no binding rule. Each rule that it names has
its owner in `docs/adr/`, or in the operator governance rules for all
repositories.

## Scope And Method

Seven read-only agents examined the documentation of the three crates. Each
agent had one area:

1. ADR 0001 to ADR 0034, and `docs/adr/archive/`.
2. ADR 0036 to ADR 0058.
3. ADR 0059 to ADR 0074, the index, the template and `.adr-dir`.
4. `docs/plans/` and `docs/vision/`.
5. `docs/tasks/`, 115 files.
6. `docs/reviews/`, `docs/security/`, `docs/schema/`,
   `docs/security-guidelines.md` and `docs/importer-review-findings.md`.
7. `AGENTS.md`, `README.md`, `GEMINI.md`, `api.html`, the reference documents
   in `docs/`, `docs/wiki/`, and the crate `AGENTS.md` and `README.md` files.

The lead examined each high and medium finding against the files and the code
before this record used it. The section "Rejected Agent Claims" gives the
claims that did not pass that check.

## Summary

The ADR set is in good condition. Each ADR file has one row in the index, and
the guard `every_adr_file_has_a_row_and_every_row_names_a_file` enforces it.
The agents found no code drift in a core decision. The API routes, the
environment variables and the module table agree with the code.

The defects are mostly outside the ADRs:

- Host details in two public files.
- A live mandate in `AGENTS.md` that points at a superseded vision plan.
- History in `AGENTS.md` "Where The Work Stands".
- Status text that did not change when the work was complete.
- Completed material that stays in the reading path.

## Priority 1: Correct Now

| ID | File | Finding | Evidence | Action |
|---|---|---|---|---|
| T1 | `AGENTS.md` | A VPS file path is in a public file. | `AGENTS.md:55` | Remove the path. Keep only the code fact. |
| T2 | `AGENTS.md` | An instruction tells the operator to examine the kernel log of the host after a crawler restart. This is an incident procedure for one host. | `AGENTS.md:83-84` | Remove it from `AGENTS.md`. Keep the check in a private operator note. |
| T3 | `docs/plans/release-0.1.0-plan.md` | Commands use the absolute path of the work directory on the developer machine. | `release-0.1.0-plan.md:162-164` | Use paths relative to the repository root, or delete the plan (T9). |
| T4 | `AGENTS.md`, `docs/vision/` | `AGENTS.md` says: "When a `docs/vision/` plan file is in flight, follow its phasing." `v4v-music-metadata-vision.md` calls itself "the single source of truth". ADR 0032 and ADR 0034 decided its phases, and ADR 0045 puts authority in the ADRs. | `AGENTS.md:713`, `AGENTS.md:789`, `docs/vision/v4v-music-metadata-vision.md:3` | Delete the mandate line and the `docs/vision/` row. Delete the five vision files, or move them out of the reading path. |
| T5 | `docs/adr/0045-governance-model-and-contract-ownership.md` | The status says that point 6, the archive and the two guards are open work. The archive exists, and `tests/adr0045_governance_guards_tests.rs` holds both guards. | ADR 0045 lines 3-6. Tests at lines 25 and 104. | Change the status to complete. Then examine if ADR 0045 can archive, because a test enforces each of its rules. |

## Priority 2: Reading Path And Ownership

| ID | File | Finding | Evidence | Action |
|---|---|---|---|---|
| T6 | `AGENTS.md` | "Where The Work Stands" is lines 7 to 346. Most of it is release history: measurements, counts, replay results and incident stories for releases 0.1.0 to 0.12.0. The governance rule says that `AGENTS.md` holds no history. | Examples: lines 52-56, 72-84, 103-107, 141-146, 289-295 | Keep the current priority, the open items and the deployed release number. Move each measurement to the review record of its ADR. The GitHub release notes and git hold the release history. |
| T7 | `docs/security-guidelines.md` | The file states rules and does not say that it is advisory. It points at the archived ADR 0018 as a design reference. Lines 124-134 give rules for proof levels, but ADR 0056 removed the proof flow. | `security-guidelines.md:10-11`, `:124-134` | Remove the proof sections and the ADR 0018 pointer. Add an advisory line that names ADR 0006, ADR 0051, ADR 0054 and ADR 0056 as owners. |
| T8 | `docs/tasks/` | Only 2 of 115 task files have a status line. The status of each other task is only in `AGENTS.md`. After T6, no file will give the status of a task. | `grep -l "^## Status" docs/tasks/*.md` gives 2 files | Choose a lifecycle (decision D2). |
| T9 | `docs/plans/` | Some plans are complete and give no open item. Examples are `release-0.1.0-plan.md`, `client-requests-work-plan.md` and `feed-trust-remediation-plan.md`. The phase plans of the complete ADRs and the request files with no open request are also complete. | Agent 4 status table | Delete each one that no ADR cites as the source of a measurement. Move each cited measurement to `docs/reviews/` first. |
| T10 | `docs/security/` | The three v1 blackbox reports say that the v2 reports supersede them. | `docs/security/auth-blackbox-report.md:6-8` | Delete the three v1 reports. |
| T11 | `docs/reviews/` | 10 review checklists are for ADRs that are complete. They have no checked box, so they are not completion records. | `ls docs/reviews/*checklist*` | Delete each checklist of a complete ADR. Keep `compression-splits-guid-review-checklist.md` until ADR 0072 to ADR 0074 are deployed. |
| T12 | `docs/importer-review-findings.md` | The file is at the root of `docs/`. It says that each issue is resolved. No document cites it. | Agent 6 finding 7 | Delete it. Before that, examine if each fix has a test. |
| T13 | `GEMINI.md` | The file gives a different mandate set, and it points at the vision plan. `.gitignore:25` excludes it, so it is not in the public repository. A Gemini agent on this machine still reads it. | `git check-ignore -v GEMINI.md` | Delete the local file, or replace its content with a pointer to `AGENTS.md`. |

## Priority 3: Small Corrections

| ID | File | Finding | Action |
|---|---|---|---|
| T14 | `stophammer-crawler/README.md` | Line 9 says "Four subcommands". The crawler has five modes. | Correct the count. |
| T15 | `AGENTS.md` | The list of restriction lints does not give `disallowed_script_idents`. `Cargo.toml:89` declares it. | Add it to the list. |
| T16 | `docs/operations.md` | `STOPHAMMER_GIT_REVISION` and `STOPHAMMER_BUILT_AT` are only in prose at line 85. They are build-time values. | Say that they are build-time values. |
| T17 | `docs/adr/README.md` | "Status Needs A Check" says that ADR 0035 uses a list status. ADR 0035 is in the archive. | Remove ADR 0035 from the note. Or change ADR 0039 and ADR 0040 to the `## Status` heading, and remove the note. |
| T18 | `docs/adr/README.md` | ADR 0057, ADR 0058, ADR 0059, ADR 0060, ADR 0062 and ADR 0067 are in "Identity, Signing And Security". ADR 0059 and ADR 0060 are API and ingest decisions. | Move each row to the section of its scope. |
| T19 | `docs/adr/0069-an-album-confirms-each-credit.md` | §1a and §3a changed for musicindex.org request 11. ADR 0068 §5 changed for the same request. The two ADRs do not point at each other. | Add a pointer in each direction. |
| T20 | `docs/reviews/` | `musicl-support-research.md`, `feed-removal-and-wavlake-forms-research.md` and `docs/schema/artist-source-evidence.md` have no citation from an ADR, a plan or `AGENTS.md`. | Examine if an ADR answers each question. If one does, delete the file. |

## Open Work That The Triage Found

These items agree with `AGENTS.md` and need no correction:

- ADR 0072, ADR 0073 and ADR 0074: each task is complete and not deployed.
  The release notes must say that community nodes upgrade first (ADR 0073 §3).
- ADR 0060 task 004: complete and not deployed.
- ADR 0074 §4: waits for the count of the feeds whose source has no
  `podcast:guid`.
- ADR 0055: Proposed until one of its two conditions is true.
- The two research items of the remaining accepted work plan: a station on a
  feed with `medium` `podcast`, and a read from a cache after a `live` podping.
- ADR 0060 §4 states that no read route gives the value block of a `musicL`
  feed. No test enforces this rule. Thus ADR 0060 stays current.

## Decisions For The Operator

- **D1.** Where does a measurement of a release go after T6? This record
  recommends the review record of the owning ADR. The other choice is the
  GitHub release notes.
- **D2.** What is the lifecycle of a task file? This record recommends that a
  task file is deleted when its ADR is complete and deployed. The ADR, the
  tests and git hold its result. The other choice is a `## Status` line in
  each task file. Before a deletion, examine the links to `docs/tasks/` from
  `AGENTS.md` and from the ADRs.
- **D3.** Delete the five `docs/vision/` files, or move them to an archive?
  The rule "dead things are deleted" supports a deletion.

## Rejected Agent Claims

The lead examined these claims, and they are not correct:

- "`GET /v1/guid-changes` is not verified." The route is at
  `src/query.rs:439` and `src/openapi.rs:390`.
- "ADR 0017 task 001 is open." Commit `af8c545` completed it.
- "ADR 0072 to ADR 0074 wait for development." Their tasks are complete and
  not deployed.
- "Audit findings F7 and F8 are not fixed." `MAX_FEED_BODY_BYTES` is at
  `stophammer-crawler/src/crawl.rs:271`. `MAX_FOLLOW_URLS_PER_FEED` and
  `MAX_FOLLOW_URLS_PER_LIST_FEED` are at `stophammer-crawler/src/follow.rs:71`
  and `:75`.
- "Audit findings F1 and F2 are open." ADR 0051, ADR 0052 and ADR 0058 decide
  them, and each one is deployed. The node stores `podcast:locked` as a source
  fact only (`src/ingest.rs:78`).
- "Audit finding F4 has no owner." ADR 0055 owns it, with the status Proposed.

## Limits

- The agents made surgical checks of the code. They did not read each source
  file.
- Agent 7 did not compare `docs/verifier-guide.md` with `build_chain()`, and
  did not compare `docs/schema-reference.md` with `src/schema.sql`.
- Agent 5 examined the acceptance criteria of 15 task files, not of all 115.
- Agent 6 did not examine audit finding F9 against the code. Its F9 statement
  is not used in this record.

## Outcome Of The Cleanup

The operator made the three decisions on 2026-10-20:

- D1: a measurement that an ADR needs moves to the review record of that ADR.
  Git keeps the other narration.
- D2: a task file is deleted when its work is deployed.
- D3: the vision files, the completed plans, the v1 security reports and the
  checklists of complete ADRs are deleted.

The line numbers in this record refer to the base commit. The cleanup did
these items:

- T1, T2 and T6: "Where The Work Stands" in `AGENTS.md` gives only the present
  state. It has no host path and no host procedure.
- T3 and T9: 16 completed plans are deleted, with `release-0.1.0-plan.md`.
  The only measurement that an ADR needs is now in
  `adr-0050-second-pass-measurement.md`, and ADR 0050 links to it. The case
  table of ADR 0064 moved to `docs/reviews/adr-0064-live-item-cases.md`.
- T4 and T13: the five vision files are deleted. The mandate now refers to a
  phase plan in `docs/plans/`. `GEMINI.md` points at `AGENTS.md`.
- T5: the status of ADR 0045 says that each point is implemented. ADR 0045
  stays current, because no test enforces its invariants on `AGENTS.md`
  content.
- T7: `docs/security-guidelines.md` says that it is advisory and names its
  owners. The proof section and the resolver section are removed.
- T8: 104 task files are deleted. The 11 files of the work that is not
  deployed stay. The test comments that named a deleted task now name the ADR
  section.
- T10, T11 and T12: the three v1 reports, 9 checklists and
  `docs/importer-review-findings.md` are deleted.
- T14, T15, T17, T18 and T19 are corrected.
- T16 needs no change. `docs/operations.md` says that the two values are set
  before the build.
- T20 needs no change. ADR 0052, ADR 0056, ADR 0057 and ADR 0060 cite the two
  research files, and `docs/API.md` and `docs/schema-reference.md` cite
  `docs/schema/artist-source-evidence.md`.
