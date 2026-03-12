# ADR 0017: Canonical RSS Parser Crate

## Status
Accepted

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
project that the TypeScript crawlers invoke via its CLI binary
(`stophammer-parse`).

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
namespace spec). The builder selects which phases to enable; only matching rules
execute. This makes adding Phase 4 support a matter of adding rules, not
rewriting parser logic.

Payment routes (`podcast:value > podcast:valueRecipient`) and value time splits
(`podcast:valueTimeSplit > podcast:remoteItem`) are nested repeated structures
that do not fit the single-field rule model. These are handled as dedicated
post-processing extractors gated on Phase 2 and Phase 3 respectively.

### CLI interface: `stophammer-parse`

The binary reads XML from stdin and writes `IngestFeedData` JSON to stdout.
TypeScript crawlers invoke it as a subprocess:

```typescript
const proc = Bun.spawn(["stophammer-parse"], { stdin: xmlBytes });
const feedData = JSON.parse(await new Response(proc.stdout).text());
```

Optional arguments:
- `--fallback-guid <guid>` — for the importer, which has GUIDs from the
  PodcastIndex database for feeds that lack `podcast:guid`
- `--phases <p1,p2,...>` — restrict which phases to enable

Exit codes: 0 success, 1 parse error (JSON error on stderr), 2 no input.

### Output types match `stophammer/src/ingest.rs`

The parser defines its own `IngestFeedData`, `IngestTrackData`,
`IngestPaymentRoute`, `IngestValueTimeSplit`, and `RouteType` types that
serialize to the same JSON shape as the stophammer ingest wire format. This
keeps the crate self-contained with no cross-crate dependency on stophammer.

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

- **Single source of truth** — all three crawlers will share the same parsing
  logic. A spec change requires updating one crate.
- **Subprocess overhead** — each feed parse spawns a process (~2 ms on modern
  hardware). Acceptable for podping (one feed at a time) and the primary
  crawler (bounded concurrency pool). For the bulk importer's 356K feeds,
  the overhead is amortised by the HTTP fetch latency that dominates each
  iteration.
- **Crawlers must ship the binary** — operators deploying crawlers need the
  `stophammer-parse` binary on PATH. The release build is a ~1.5 MB static
  binary (musl, LTO, stripped).
- **ADR 0011 is not superseded** — the RSS crawler's runtime (Bun), scheduling
  model, content hash strategy, and `POST /ingest/feed` protocol remain as
  decided. Only the XML parsing step changes from inline TypeScript to a
  subprocess call.
