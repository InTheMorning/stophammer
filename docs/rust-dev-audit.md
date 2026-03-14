# Rust Dev Audit -- Microsoft Pragmatic Rust Guidelines

**Date:** 2026-03-13
**Scope:** All source files in `/Volumes/T7/hey-v4v/stophammer/src/`
**Reference:** Microsoft Pragmatic Rust Guidelines (error handling, API design, async, safety, docs, performance, lints)

---

## CRITICAL violations (prevents correct operation or has security impact)

### CRIT-01 -- M-STATIC-VERIFICATION: No `[lints]` table in `Cargo.toml`

- **File:** `Cargo.toml` (entire file)
- **Severity:** CRITICAL
- **Snippet:** `Cargo.toml` contains no `[lints.rust]` or `[lints.clippy]` section.
- **What's wrong:** The guidelines mandate project-level lint configuration enabling `pedantic`, `correctness`, `style`, `suspicious`, `perf`, `cargo`, `complexity` clippy groups, plus specific `restriction` lints (`map_err_ignore`, `clone_on_ref_ptr`, `string_to_string`, `undocumented_unsafe_blocks`, etc.) and compiler lints (`missing_debug_implementations`, `unsafe_op_in_unsafe_fn`, etc.). The project currently relies only on `#![warn(clippy::pedantic)]` in `lib.rs` and `main.rs`, missing the bulk of the mandated lint surface.
- **Fix:** Add the full `[lints.rust]` and `[lints.clippy]` tables from the guidelines to `Cargo.toml`. Remove the per-file `#![warn(clippy::pedantic)]` attributes (they become redundant).

### CRIT-02 -- M-TEST-UTIL: `skip_ssrf_validation` field is not feature-gated

- **File:** `src/api.rs:303`, `src/main.rs:76,173`
- **Severity:** CRITICAL
- **Snippet:**
  ```rust
  pub skip_ssrf_validation: bool,
  ```
- **What's wrong:** `M-TEST-UTIL` requires that safety-check overrides be guarded behind a feature flag (e.g. `#[cfg(feature = "test-util")]`). The `skip_ssrf_validation` field is a production-compiled boolean that, if accidentally set to `true`, disables SSRF validation for the proof-of-possession assertion flow. All 15+ test files set it to `true`. It should not even be compilable in release builds.
- **Fix:** Gate `skip_ssrf_validation` behind `#[cfg(any(test, feature = "test-util"))]`. In production, the field is absent and the code path always validates. Provide a `#[cfg(not(...))]` stub that is always `false`.

### CRIT-03 -- M-PUBLIC-DEBUG: Multiple public types missing `Debug`

- **File:** `src/api.rs:282` (`AppState`), `src/api.rs:310` (`ApiError`), `src/db.rs:73` (`EventRow`), `src/db.rs:95` (`ExternalIdRow`), `src/db.rs:104` (`EntitySourceRow`), `src/apply.rs:15` (`ApplyOutcome`), `src/apply.rs:25` (`ApplySummary`), `src/signing.rs:20` (`NodeSigner`), `src/verify.rs:43` (`IngestContext`), `src/verify.rs:100` (`VerifierChain`), `src/verify.rs:148` (`ChainSpec`), `src/community.rs:46` (`CommunityConfig`), `src/community.rs:62` (`CommunityState`), `src/proof.rs:17` (`ChallengeRow`), `src/search.rs:37` (`SearchResult`), `src/quality.rs:19,29,181` (internal structs)
- **Severity:** CRITICAL
- **What's wrong:** `M-PUBLIC-DEBUG` and `C-COMMON-TRAITS` require all public types to derive or implement `Debug`. Many public structs lack `#[derive(Debug)]`.
- **Fix:** Add `#[derive(Debug)]` to every public type. For `NodeSigner` (which contains secret key material), implement a custom `Debug` that redacts the signing key.

---

## HIGH violations (guideline violations with functional impact)

### HIGH-01 -- M-LOG-STRUCTURED: Logging uses tracing macros but not named events or OTel conventions

- **File:** All files using `tracing::info!`, `tracing::warn!`, `tracing::error!`, `tracing::debug!`
- **Examples:** `src/community.rs:142`, `src/api.rs:1262`, `src/main.rs:196`, `src/tls.rs:175`, `src/proof.rs:263`
- **Severity:** HIGH
- **What's wrong:** `M-LOG-STRUCTURED` requires:
  1. Named events using `name:` parameter with hierarchical dot-notation (`<component>.<operation>.<state>`)
  2. Message templates with `{{property}}` syntax instead of string formatting
  3. Following OTel semantic conventions for attributes

  Current code uses ad-hoc string messages like `tracing::info!(primary = %primary_url, "community: registered push endpoint with primary")` without `name:` and with direct string interpolation.
- **Fix:** Add `name:` parameters to all tracing events. Use message template syntax. Adopt OTel-aligned attribute names (e.g. `db.operation.name`, `error.type`).

### HIGH-02 -- M-STRONG-TYPES: File paths passed as `&str` instead of `Path`/`PathBuf`

- **File:** `src/signing.rs:82` (`load_or_create(path: &str)`), `src/db.rs:130` (`open_db(path: &str)`), `src/tls.rs:302` (`write_private_file(path: &str, ...)`), `src/tls.rs:36` (`cert_needs_renewal(cert_path: &str)`), `src/tls.rs:23-29` (`TlsConfig` fields)
- **Severity:** HIGH
- **Snippet:**
  ```rust
  pub fn load_or_create(path: &str) -> Result<Self, SigningError> {
  ```
- **What's wrong:** `M-STRONG-TYPES` mandates using `Path`/`PathBuf` for anything dealing with the OS filesystem, not `String`/`&str`.
- **Fix:** Change all filesystem path parameters to `&Path` or `impl AsRef<Path>`. Change `TlsConfig` string path fields to `PathBuf`.

### HIGH-03 -- M-CANONICAL-DOCS: Many public functions lack `# Examples` section

- **File:** Nearly all public functions across `src/db.rs`, `src/proof.rs`, `src/signing.rs`, `src/search.rs`, `src/quality.rs`, `src/api.rs`, `src/verify.rs`
- **Severity:** HIGH
- **What's wrong:** `M-CANONICAL-DOCS` and `C-EXAMPLE` strongly encourage examples in documentation. No public function in the codebase has a `# Examples` doc section.
- **Fix:** Add `# Examples` with runnable code snippets to the most important public functions (at minimum `open_db`, `NodeSigner::load_or_create`, `VerifierChain::run`, `apply_single_event`, `create_challenge`, `validate_token`, `search`).

### HIGH-04 -- M-DOCUMENTED-MAGIC: Magic numbers used inline without named constants

- **File:** `src/proof.rs:58` (`86400`), `src/proof.rs:142` (`3600`), `src/community.rs:204` (`Duration::from_secs(2)`), `src/tls.rs:184` (`12 * 60 * 60`), `src/api.rs:2193` (`3600`)
- **Severity:** HIGH
- **Snippet:**
  ```rust
  let expires_at = now + 86400; // 24 hours
  ```
- **What's wrong:** `M-DOCUMENTED-MAGIC` requires magic values to be named constants with documentation explaining why the value was chosen, non-obvious side effects, and interacting systems.
- **Fix:** Extract to named constants, e.g. `const CHALLENGE_TTL_SECS: i64 = 86400;` with a doc comment explaining the rationale. Apply to all inline durations.

### HIGH-05 -- M-MIMALLOC-APPS: Application uses mimalloc (COMPLIANT) but `lib.rs` also declares `#![warn(clippy::pedantic)]`

This is noted as part of CRIT-01. The mimalloc usage in `main.rs` is correct per `M-MIMALLOC-APPS`.

### HIGH-06 -- M-PANIC-IS-STOP / M-PANIC-ON-BUG: Several `.expect()` calls on recoverable errors

- **File:** `src/main.rs:47` (`expect("CRAWL_TOKEN env var required")`), `src/main.rs:54` (`expect("db mutex poisoned at startup")`), `src/main.rs:322` (`.unwrap()` on `axum::serve`), `src/main.rs:327` (`.unwrap()` on `TcpListener::bind`), `src/community.rs:101` (`expect("failed to build reqwest client")`)
- **Severity:** HIGH
- **What's wrong:** While some of these are arguably programming errors (per `M-PANIC-ON-BUG`), the `unwrap()` calls on network bind and serve operations at `main.rs:322,327,331` are recoverable runtime errors (port in use, permission denied), not programming bugs. Using `unwrap()` there violates `M-PANIC-IS-STOP`.
- **Fix:** For the startup-critical panics (env vars, TLS loading), add descriptive `expect` messages that explain the fix. For `.unwrap()` on `axum::serve` and `TcpListener::bind`, handle the error with a tracing::error + process::exit(1) pattern, or use `expect` with a clear message explaining the unrecoverability.

### HIGH-07 -- M-APP-ERROR: Mixed error types; should standardize on one application error crate

- **File:** `src/api.rs` (custom `ApiError`), `src/db.rs` (custom `DbError`), `src/signing.rs` (custom `SigningError`), `src/verify.rs` (returns `Result<..., String>`)
- **Severity:** HIGH
- **What's wrong:** `M-APP-ERROR` states that once you select an application error crate you should switch all application-level errors to that type and not mix multiple types. The codebase has three custom error types plus raw `String` errors. While `M-APP-ERROR` permits custom types (it relaxes `M-ERRORS-CANONICAL-STRUCTS`), using `String` as an error type in `verify.rs` and `proof.rs` is unstructured and does not implement `std::error::Error`.
- **Fix:** Either adopt `anyhow`/`eyre` across the application, or at minimum replace `Result<..., String>` with proper typed errors that implement `Error`. The custom `DbError` / `SigningError` / `ApiError` are reasonable if they remain the only three.

---

## MEDIUM violations (style / maintainability)

### MED-01 -- M-FIRST-DOC-SENTENCE: Some doc comments exceed ~15 words in the summary sentence

- **File:** `src/community.rs:92` (23 words), `src/proof.rs:233-246` (multi-line first sentence), `src/tls.rs:55` (25 words)
- **Severity:** MEDIUM
- **What's wrong:** `M-FIRST-DOC-SENTENCE` recommends the summary sentence be approximately 15 words maximum.
- **Fix:** Shorten summary sentences to under 15 words, moving detail into the extended documentation paragraph.

### MED-02 -- M-CONCISE-NAMES: `SseConnectionGuard` contains the word pattern but is acceptable

- **File:** `src/api.rs:1433`
- **Severity:** MEDIUM (borderline -- `Guard` is an established Rust RAII pattern, not a weasel word)
- **What's wrong:** `M-CONCISE-NAMES` warns against weasel words like `Manager`, `Service`, `Factory`. `Guard` is an established Rust term (e.g. `MutexGuard`). No action needed.
- **Fix:** None required.

### MED-03 -- M-LINT-OVERRIDE-EXPECT: `#[expect]` usage is correct but inconsistent with module-level `#![warn]`

- **File:** `src/lib.rs:1`, `src/main.rs:1`
- **Severity:** MEDIUM
- **Snippet:**
  ```rust
  #![warn(clippy::pedantic)]
  ```
- **What's wrong:** Per `M-LINT-OVERRIDE-EXPECT` and `M-STATIC-VERIFICATION`, lint configuration should live in `Cargo.toml` `[lints]`, not in source files. The `#![warn(clippy::pedantic)]` in both `lib.rs` and `main.rs` is redundant once `Cargo.toml` lints are configured and can become stale.
- **Fix:** Move to `Cargo.toml` `[lints.clippy]` and remove from source files.

### MED-04 -- M-MODULE-DOCS: `src/lib.rs` has no module documentation

- **File:** `src/lib.rs:1-19`
- **Severity:** MEDIUM
- **What's wrong:** `M-MODULE-DOCS` requires `//!` module documentation on all public modules. `lib.rs` is the crate root and has no `//!` docs describing the crate's purpose, architecture, or contained modules.
- **Fix:** Add crate-level `//!` documentation to `src/lib.rs` describing stophammer's purpose, module layout, and key entry points.

### MED-05 -- M-MODULE-DOCS: `src/main.rs` has no module documentation

- **File:** `src/main.rs:1-7`
- **Severity:** MEDIUM
- **What's wrong:** While `main.rs` is not a library module, the guideline spirit suggests documentation for orientation. The file lacks any `//!` doc.
- **Fix:** Add a brief `//!` doc comment to `main.rs`.

### MED-06 -- M-MODULE-DOCS: `src/apply.rs` has no crate-level `//!` doc on first line

- **File:** `src/apply.rs:1`
- **Severity:** MEDIUM (actually present at line 1-6 as `//!` comments -- COMPLIANT)
- **Fix:** None required.

### MED-07 -- C-COMMON-TRAITS: Domain model types missing `Eq`, `Hash`, `PartialEq` where applicable

- **File:** `src/model.rs` -- `Artist`, `Feed`, `Track`, `ArtistCredit`, `ArtistCreditName`, `ValueTimeSplit`, `FeedPaymentRoute`, `PaymentRoute`
- **Severity:** MEDIUM
- **What's wrong:** `C-COMMON-TRAITS` recommends types eagerly implement common traits. Most model types only derive `Debug, Clone, Serialize, Deserialize`. They lack `PartialEq`, `Eq`, and `Hash` which would be useful for testing and deduplication.
- **Fix:** Add `#[derive(PartialEq, Eq)]` to all model types that do not contain floating-point fields (none of them do -- `split` is `i64`). Add `Hash` where it makes sense (e.g. types used as map keys).

### MED-08 -- M-REGULAR-FN: Free functions in `db.rs` are correctly not associated -- COMPLIANT

- **File:** `src/db.rs`
- **Severity:** N/A
- **Fix:** None required. The `db` module correctly uses free functions rather than putting everything in an `impl Db` block.

### MED-09 -- M-YIELD-POINTS: `apply_events` loop has correct yield points -- COMPLIANT with note

- **File:** `src/apply.rs:227-266`
- **Severity:** MEDIUM (informational)
- **What's wrong:** The loop contains `spawn_blocking` and `.await` per iteration, which provides yield points. However, each iteration spawns a new `spawn_blocking` task for a single event. For large batches (1000 events), this creates 1000 blocking task spawns. Batching the DB writes could improve throughput per `M-THROUGHPUT`.
- **Fix:** Consider batching events into a single `spawn_blocking` call with a transaction, yielding every N events. Not a guideline violation, but a performance recommendation.

### MED-10 -- M-SMALLER-CRATES: Monolithic single crate

- **File:** `Cargo.toml`
- **Severity:** MEDIUM
- **What's wrong:** `M-SMALLER-CRATES` recommends splitting crates when submodules can be used independently. The `model`, `event`, `sync`, `verify`, `search`, and `quality` modules are independently usable and could be separate crates to improve compile times.
- **Fix:** Consider extracting `stophammer-model`, `stophammer-event`, `stophammer-verify` as internal crates. Low priority for an application binary.

### MED-11 -- M-HOTPATH: No benchmarks exist

- **File:** N/A (no `benches/` directory)
- **Severity:** MEDIUM
- **What's wrong:** `M-HOTPATH` recommends identifying hot paths early and creating benchmarks. The ingest handler (`handle_ingest_feed`) and FTS5 search (`search::search`) are the hot paths; neither has benchmarks.
- **Fix:** Add `criterion` or `divan` benchmarks for the ingest transaction and FTS5 search operations.

---

## TODOs / stubs still in source

### TODO-01 -- Grep result: `Phase 2` comment in api.rs

- **File:** `src/api.rs:2132`
- **Snippet:** `// ── Phase 2 (async): fetch RSS and verify podcast:txt ─────────────────────`
- **Status:** This is a section label, not a TODO/stub. The Phase 2 code is fully implemented (RSS fetch + podcast:txt verification). **Not a violation.**

### TODO-02 -- Grep result: `skip_ssrf` references

- **File:** `src/api.rs:303,2135`, `src/main.rs:76,173`, and 15+ test files
- **Status:** Covered by CRIT-02 above. The `skip_ssrf_validation` field is a test-only safety override that must be feature-gated.

### TODO-03 -- Grep result: test comment referencing old TODO

- **File:** `tests/crypto_security_tests.rs:512`
- **Snippet:** `/// "// TODO: fetch RSS at feed_url and verify podcast:txt token before issuing -- Phase 2"`
- **Status:** This is a test comment documenting that the old TODO has been completed. The code is implemented. **Not an active stub.**

**Summary:** Zero active `todo!()`, `unimplemented!()`, `FIXME`, `HACK`, or `#[ignore]` markers found in source. The `skip_ssrf_validation` field is the only testing escape hatch that needs feature-gating.

---

## Already compliant (brief list)

| Guideline | Status | Notes |
|---|---|---|
| **M-MIMALLOC-APPS** | COMPLIANT | `main.rs:3-6` uses mimalloc as global allocator |
| **M-APP-ERROR** | MOSTLY COMPLIANT | Custom `DbError`, `SigningError`, `ApiError` are reasonable for an application. `String` errors in `verify.rs`/`proof.rs` are the exception (HIGH-07). |
| **M-MODULE-DOCS** | MOSTLY COMPLIANT | 14 of 17 modules have `//!` docs. Missing on `lib.rs` and `main.rs` (MED-04, MED-05). |
| **M-CANONICAL-DOCS** | MOSTLY COMPLIANT | Public functions have `# Errors` and `# Panics` sections where applicable. Missing `# Examples` (HIGH-03). |
| **M-UNSAFE** | COMPLIANT | Zero `unsafe` blocks in the entire codebase. |
| **M-UNSOUND** | COMPLIANT | No unsound abstractions detected. |
| **M-PANIC-ON-BUG** | MOSTLY COMPLIANT | `expect()` messages are descriptive. Some `unwrap()` calls on runtime errors (HIGH-06). |
| **M-LINT-OVERRIDE-EXPECT** | COMPLIANT | All clippy overrides use `#[expect(..., reason = "...")]` with reasons. |
| **M-TYPES-SEND** | COMPLIANT | All public types are `Send` + `Sync` compatible. `Arc<Mutex<Connection>>` is the DB handle. |
| **M-YIELD-POINTS** | COMPLIANT | Community sync loop and ingest handlers contain `.await` yield points. Comment in `community.rs:131-136` documents the strategy. |
| **M-THROUGHPUT** | COMPLIANT | DB operations are batched via `ingest_transaction`. `spawn_blocking` isolates CPU-bound work. |
| **M-REGULAR-FN** | COMPLIANT | Database operations are free functions, not associated methods on a wrapper type. |
| **M-CONCISE-NAMES** | COMPLIANT | No `Manager`, `Service`, or `Factory` types. Names are domain-specific. |
| **M-DOC-INLINE** | N/A | No `pub use` re-exports to inline. |
| **M-NO-GLOB-REEXPORTS** | COMPLIANT | No `pub use foo::*` anywhere. |
| **M-FEATURES-ADDITIVE** | N/A | No cargo features defined. |
| **M-AVOID-STATICS** | COMPLIANT | Only the `#[global_allocator]` static exists (mimalloc, per guidelines). |
| **M-DONT-LEAK-TYPES** | N/A | Application binary, not a library. |
| **M-ISOLATE-DLL-STATE** | N/A | No DLL/FFI boundaries. |
| **M-ESCAPE-HATCHES** | N/A | No native handle wrappers. |
| **M-AVOID-WRAPPERS** | N/A | Application binary, not a library API. |

---

## Priority summary for sprint planning

| Priority | Count | Key items |
|---|---|---|
| CRITICAL | 3 | Cargo.toml lints (CRIT-01), feature-gate `skip_ssrf_validation` (CRIT-02), `Debug` derives (CRIT-03) |
| HIGH | 7 | Structured logging (HIGH-01), Path types (HIGH-02), doc examples (HIGH-03), magic numbers (HIGH-04), panic hygiene (HIGH-06), error type consistency (HIGH-07) |
| MEDIUM | 9 | lib.rs docs (MED-04), model traits (MED-07), benchmarks (MED-11), etc. |
| TODOs/stubs | 0 active | `skip_ssrf` is a feature-gate issue (CRIT-02), not a stub |
