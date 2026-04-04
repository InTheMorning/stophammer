# V4V Music Metadata Vision & Master Refactoring Plan

This document serves as the single source of truth for transitioning the `stophammer` codebase.

## Phase 0: Governance & CI Compliance
To ensure long-term maintainability and model-efficiency, all work must adhere to the following standards:

**1. ADR (Architectural Decision Record) Methodology**
*   Before implementing Phase 3 (Ingest) or Phase 4 (CLI Tool), the AI must create a new ADR in `docs/adr/` (e.g., `0031-lineage-aware-ingest.md`).
*   The ADR must detail the schema changes and the specific heuristic logic to be used.

**2. Mandatory CI Validation (The "Green Commit" Rule)**
*   **Verification:** After every file edit, the AI MUST execute `cargo check` and `cargo fmt --check`.
*   **Zero-Warning Policy:** No code changes shall be considered complete if they introduce new compiler warnings or lint errors (`clippy`).
*   **Test-Driven Execution:** Phase 2 and 3 changes must include a corresponding integration test in `tests/` that demonstrates the new lineage/skip logic working on a mock RSS feed.

**3. Token-Optimized Workflow**
*   **Mechanical Tasks:** Use low-tier models for Phase 1 and 2.
*   **Surgical Edits:** Only provide the AI with the specific functions it needs to change, not the entire file, to keep the context window small and context-costs low.

## Phase 1: Clean Slate & De-bloating
Before adding new features, we must remove the technical debt of remote API TUI access and obsolete planning files.

**1. Remove Obsolete Planning Files (DONE)**
*   **Result:** Fragmented plans (`docs/*-plan.md`) have been removed. This document is now the sole planning artifact.

**2. Audit & Remove Remote TUI API Hooks**
*   **Target Files:** `src/api.rs`, `src/query.rs`, `src/review_backend.rs`.
*   **Action:** Delete `src/review_backend.rs` entirely.
*   **Action:** Remove the `with_admin_review_routes` function and its associated handlers (e.g., `handle_admin_feed_evidence`, `handle_admin_resolve_wallet_identity_review`, `handle_admin_wallet_apply_merges`) from `src/api.rs`.
*   **Action:** Remove `ApiBackend` and `DbBackend` trait implementations. The TUIs will revert to direct, high-performance SQLite access.

## Phase 2: Importer Optimization
The PodcastIndex importer (`stophammer-crawler/src/modes/import.rs`) must be optimized to save days of ingestion time and bandwidth.

**1. Music-First Skip (Hardcoded)**
*   **Target:** `stophammer-crawler/src/modes/import.rs` (in the `run` function).
*   **Action:** Define `const MIN_MUSIC_FEED_ID: i64 = 4630863;`.
*   **Action:** Ensure `cursor` is initialized to at least `MIN_MUSIC_FEED_ID`. If `cursor < MIN_MUSIC_FEED_ID`, log a jump message and set `cursor = MIN_MUSIC_FEED_ID`. This skips 4.6 million irrelevant, non-music rows.

**2. Snapshot Staleness Detection (Conditional GET)**
*   **Target:** `stophammer-crawler/src/modes/import.rs` (`ensure_snapshot_db` function).
*   **Action:** Instead of unconditionally downloading `podcastindex_feeds.db.tgz` when `refresh_db` is false, inspect the local file's modification time (`fs::metadata(db_path)?.modified()`).
*   **Action:** Use `reqwest` to issue a GET request with the `If-Modified-Since` header.
*   **Action:** If the server returns HTTP 304 (Not Modified), skip the download. If HTTP 200, stream the download and extract the archive.
*   **Action:** Store the latest `mtime` in the `import_progress` table using `ProgressStore::set_last_id` (or a new dedicated method) to survive container restarts.

## Phase 3: Thorough & Lineage-Aware Ingest
The parser must capture *all* Podcasting 2.0 tags without prematurely prioritizing one over another.

**1. Schema Expansion (`src/schema.sql`)**
*   **Action:** Add `generator TEXT` and `generator_lineage TEXT` to the `feeds` table definition.
*   **Action:** Update `src/model.rs` -> `struct Feed` to include `generator: Option<String>` and `generator_lineage: Option<String>`.

**2. Lineage Filter implementation**
*   **Target:** `stophammer-parser/src/` or `src/ingest.rs` (wherever the XML is transformed into `IngestFeedData`).
*   **Action:** Extract the raw string from `<generator>`.
*   **Action:** Implement `fn determine_lineage(generator: Option<&str>) -> String`:
    *   Contains "Wavlake" -> `"wavlake"`
    *   Contains "Music Side Project" and URL `new.musicsideproject.com` or v2 -> `"msp_2"`
    *   Contains "Music Side Project" (v1) -> `"msp_1"`
    *   Empty/matches `feed-with-comments.xml` -> `"demu"`
    *   Else -> `"unknown"`

**3. "Equal Weight Evidence" Parsing Strategy**
*   **Target:** `src/db.rs` -> `ingest_transaction`.
*   **Action:** Do not discard `itunes:author` in favor of `podcast:person`. Extract *both* into `source_contributor_claims`.
*   **Action:** Ensure `<podcast:person>` extraction captures `href`, `img`, `group`, and `role` identically in `SourceContributorClaim`.
*   **Action:** For Wavlake feeds, prioritize `remoteItem` GUIDs as definitive linkage keys, acknowledging they indicate *publishership*, not necessarily *artist identity*.
*   **Action:** For MSP 2.0 feeds, detect the `<podcast:publisher>` tag and link the album feed to the publisher identity immediately.
*   **Action:** For "De-Mu" legacy feeds, explicitly capture `role="band"` alongside `itunes:author`. Repurpose `<podcast:episode>` as `track_number`.

## Phase 4: The "Visible & Tweakable" Resolver Workflow
We will replace silent "black box" merging with an evidence-based Review system.

**1. Conflict Surfacing**
*   **Target:** `src/db.rs` -> `sync_canonical_state_for_feed` or related canonicalization logic.
*   **Action:** When evaluating `source_contributor_claims` to determine the "Album Artist" or "Track Artist", check the `generator_lineage`.
*   **Action:** If `itunes:author` and `podcast:person` present highly divergent strings (e.g., "John" vs "The John Doe Trio") and the lineage does not implicitly trust one over the other, **do not merge**.
*   **Action:** Instead, insert a record into `artist_identity_review` (or a similar table) with status `pending`, surfacing the conflicting claims as `evidence`.

**2. The Resolver "What-If" CLI Tool (`stophammer-resolver-debug`)**
To provide the "Visibility" required to trust the data, this tool will focus on **Evidence Visualization** rather than just result reporting.

*   **Action:** Implement a CLI binary in `src/bin/stophammer_resolver_debug.rs` that accepts target Feed GUIDs or Artist IDs.
*   **Action:** It runs the heuristic matching logic (e.g., fuzzy title match, duration +/- 2s) *without* calling `tx.commit()`.
*   **Action:** It outputs a comparative table showing:
    *   **Artist Identity:** Side-by-side comparison of `itunes:author` vs. `podcast:person` (including role/group) across all source feeds.
    *   **Track Metadata:** Verbatim titles and `itunes:duration` (to the second) to spot drift between encoders.
    *   **Temporal Context:** `release_date` (derived from `pubDate`) and `last_updated` timestamps for every source.
    *   **"Appears On" Lineage:** A complete list of every Feed GUID claiming the track, categorized by its `generator_lineage` (e.g., "This track appears on 1 Wavlake Album and 3 Hand-Hacked Radio Shows").
*   **Action:** Provide sensitivity analysis flags: `--duration-tolerance=5`, `--title-fuzzy-threshold=0.8`, or `--ignore-itunes-author` to let operators visualize changes before applying them.
*   **Action:** Conflict Flags: Any recording that has divergent durations (>3s) or divergent artist strings across feeds MUST be flagged with a "SKEPTICAL" status, requiring manual review.
