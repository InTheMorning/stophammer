# ADR 0017: Canonical RSS Parser Crate

## Status
Accepted. Amended on 2026-10-20: the crawler calls the parser as a
library, and the CLI stays for operators and tests.

## Context
ADR 0011 chose `fast-xml-parser` in TypeScript for the primary RSS crawler.
ADR 0012 and ADR 0013 introduced two more crawlers (podping listener and bulk
importer) that each implemented their own XML parsing — the podping and importer
crawlers use regex-based extraction while the primary crawler uses a proper XML
library.

Sprint 5 exposed the cost of this duplication: five parsing bugs had to be fixed
independently in all three codebases:

1. `route_type` defaulted to "lightning" instead of "node"
2. `podcast:valueTimeSplit` read GUIDs from attributes on the split element
   instead of the `podcast:remoteItem` child element
3. `itunes:explicit` only accepted "yes", not "true"
4. Feed-level `podcast:value` recipients were not extracted
5. `itunes:owner > itunes:name` nesting was not handled

Any future podcast namespace spec change (e.g. Phase 4 `podcast:person`) would
require the same fix in three places.

## Decision

### Single Rust library crate: `stophammer-parser`

A standalone Rust library crate provides the canonical RSS/Podcast XML parser.
The crate is **not a dependency of the stophammer binary** — it is a separate
project. `stophammer-crawler` calls it as a library,
through a path dependency. The CLI binary `stophammer-parse` stays for
operators and tests.

### Why Rust, not a shared TypeScript module

The parser is a pure function (XML bytes in, structured data out, no I/O). Rust
provides three advantages over a shared TypeScript module:

1. **Correctness via types** — `Phase`, `Transform`, `Source`, `Target` enums
   make illegal states unrepresentable. The `ParseError` struct distinguishes
   malformed XML from missing required fields.
2. **Speed for batch imports** — the bulk importer processes ~356K candidate
   feeds. A compiled parser with `roxmltree` (zero-copy DOM) is measurably
   faster than `fast-xml-parser` in Bun.
3. **Single binary distribution** — operators deploy one static binary. No
   `node_modules`, no runtime version management.

### Why not merge into the stophammer binary

ADR 0006 established that crawlers are untrusted clients communicating via
`POST /ingest/feed`. The parser belongs on the crawler side of that boundary.
The stophammer binary validates and stores; it does not parse XML. Merging the
parser into stophammer would violate this separation.

### Declarative rule-based extraction

The parser uses a rule-based engine inspired by declarative extraction patterns.
Each rule declares three things:

- **Source** — where to find a value in the XML DOM (child text, attribute,
  nested element)
- **Transform** — how to process the extracted string (parse date, parse
  duration, strip HTML, decode entities, parse explicit flag)
- **Target** — which field on the output struct to populate

Rules are grouped by **Phase** (RSS 2.0 Core, iTunes, Phase 1–6 of the podcast
namespace spec). The builder selects which normalized extractors to enable; only
matching rules execute. This makes adding new typed support a matter of adding
rules, not rewriting parser logic.

Separately, the parser now preserves the complete Podcast Namespace 1.0 XML
surface in a generic `podcast_namespace` snapshot on `IngestFeedData`. That
snapshot is prefix-agnostic, accepts both the canonical namespace URI and the
legacy GitHub-doc URI that the spec declares equivalent, and ensures callers do
not lose tags that have not yet been promoted into dedicated typed fields.

Payment routes (`podcast:value > podcast:valueRecipient`) and value time splits
(`podcast:valueTimeSplit > podcast:remoteItem`) are nested repeated structures
that do not fit the single-field rule model. These are handled as dedicated
post-processing extractors gated on Phase 2 and Phase 3 respectively.

### CLI interface: `stophammer-parse`

The binary reads XML from stdin and writes `IngestFeedData` JSON to stdout.
An operator uses it to examine one feed.

Optional arguments:
- `--phases <p1,p2,...>` — restrict which phases to enable

Exit codes: 0 success, 1 parse error (JSON error on stderr), 2 no input.

### Output types match `stophammer/src/ingest.rs`

The parser defines its own `IngestFeedData`, `IngestTrackData`,
`IngestPaymentRoute`, `IngestValueTimeSplit`, and `RouteType` types that
serialize to the same JSON shape as the stophammer ingest wire format. It also
adds a crawler-side `podcast_namespace` snapshot containing every Podcast
Namespace 1.0 element seen in the source XML. This keeps the crate
self-contained with no cross-crate dependency on stophammer while still
retaining the full RSS namespace surface.

### Dependencies

- `roxmltree` 0.21 — pure Rust, zero-copy DOM parser. No system dependencies,
  no C bindings. Handles namespaces natively: `podcast:guid` resolves to
  local name `guid` with namespace URI
  `https://podcastindex.org/namespace/1.0`.
- `chrono` 0.4 — RFC-2822 and ISO-8601 date parsing to Unix seconds.
- `regex` 1 — HTML tag stripping for description fields.
- `serde` / `serde_json` — optional (default-on) feature for JSON
  serialization of output types.

## Consequences

- **Single source of truth** — each crawler mode shares the same parsing logic.
  A spec change requires updating one crate.
- **No subprocess** — the crawler calls the parser in its own process.
  A parser change needs a new build of the crawler.
- **ADR 0011 is archived** — ADR 0072, ADR 0073 and ADR 0074 supersede
  its last sections in force. ADR 0073 §5 names the owner of each other
  section.
