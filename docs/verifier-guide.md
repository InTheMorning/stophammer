# Verifier Development Guide

This guide explains how the verifier chain works and how to create custom verifiers for the Stophammer primary node.

---

## What the Verifier Chain Is

The verifier chain is an ordered set of quality checks. The primary node runs it on each `POST /ingest/feed` submission, after it checks the crawl token. Each verifier reads the incoming feed data and gives a pass, a warning, or a reject.

The crawl token check comes first. It is not a member of this chain. `VerifierChain::new` holds the token apart from the configurable list. `VerifierChain::authenticate` checks the token before the node reads the database, before the chain runs, and before the content-hash shortcut. No `VERIFIER_CHAIN` value can add, remove or reorder this check ([ADR 0051](adr/0051-feed-content-comes-from-its-source-url.md) section 4).

The chain is the core quality gate: it is the reason Stophammer's index
primarily contains verified V4V music feeds, while still allowing selected
container/source-layer mediums (`publisher`, `musicL`) through with narrower
semantics. Community nodes do NOT run verifiers -- they verify the ed25519
signature on events and trust the primary's verification result. Verifier
warnings from the primary are stored in each event's `warnings` field as an
audit trail replicated to all nodes.

Architecture decision: [ADR-0015 -- Verifier Plugin Architecture](adr/0015-verifier-plugin-architecture.md).

---

## The Verifier Trait

Every verifier implements the `Verifier` trait defined in `src/verify.rs`:

```rust
pub trait Verifier: Send + Sync {
    /// Short identifier used in warning and rejection messages.
    fn name(&self) -> &'static str;

    /// Run this check against `ctx` and return the outcome.
    fn verify(&self, ctx: &IngestContext) -> VerifyResult;
}
```

`Send + Sync` is required because the verifier chain is shared across async tasks (wrapped in `Arc<VerifierChain>`).

---

## IngestContext Fields

The `IngestContext` struct provides all the data a verifier needs:

```rust
pub struct IngestContext<'a> {
    /// The ingest request being validated, including parsed feed data.
    pub request:  &'a IngestFeedRequest,

    /// Read-only database connection for verifiers that need prior state.
    pub db:       &'a Connection,

    /// The feed row already stored for this URL, if one exists.
    pub existing: Option<&'a Feed>,
}
```

### `request` (`IngestFeedRequest`)

| Field | Type | Description |
|-------|------|-------------|
| `canonical_url` | `String` | The feed URL after redirect resolution |
| `source_url` | `String` | The original URL before redirects |
| `crawl_token` | `String` | The crawler's authentication token |
| `http_status` | `u16` | HTTP status returned by the crawler |
| `content_hash` | `String` | SHA-256 hex hash of the feed body |
| `feed_data` | `Option<IngestFeedData>` | Parsed feed content (None if fetch failed) |

When `feed_data` is `Some`, it contains:

| Field | Type | Description |
|-------|------|-------------|
| `feed_guid` | `String` | `podcast:guid` value |
| `title` | `String` | Feed title |
| `description` | `Option<String>` | Feed description |
| `raw_medium` | `Option<String>` | `podcast:medium` tag value (e.g. `"music"`, `"publisher"`, `"musicL"`) |
| `author_name` | `Option<String>` | Feed author |
| `owner_name` | `Option<String>` | Feed owner |
| `feed_payment_routes` | `Vec<IngestPaymentRoute>` | Feed-level V4V payment routes |
| `tracks` | `Vec<IngestTrackData>` | Per-episode data including per-track routes |

### `db` (`&Connection`)

A read-only SQLite connection. Verifiers that need prior state (e.g. `content_hash` checking the crawl cache) query the database directly. Write operations should NOT be performed in verifiers.

### `existing` (`Option<&Feed>`)

The feed row already stored for this URL, if one exists. Useful for verifiers that need to diff against prior state (e.g. detecting field changes). Currently populated but not consumed by any built-in verifier -- available for custom verifiers.

---

## VerifyResult Variants

```rust
pub enum VerifyResult {
    /// The check passed; ingestion continues normally.
    Pass,

    /// The check raised a concern but did not block ingestion.
    /// The message is stored with the event record for later audit.
    Warn(String),

    /// The check failed; ingestion is rejected.
    /// The message is returned to the crawler as the rejection reason.
    Fail(String),
}
```

**Chain behavior:**
- `Pass` -- continues to the next verifier
- `Warn(msg)` -- continues; the message is collected and stored in the event's `warnings` field
- `Fail(msg)` -- stops the chain immediately; the message is returned to the crawler

Warning messages are formatted as `[verifier_name] message` automatically by the chain runner.

---

## How to Configure via VERIFIER_CHAIN

The `VERIFIER_CHAIN` environment variable controls which quality verifiers run and in what order. It is a comma-separated list of verifier names. `crawl_token` is not a correct name here ([ADR 0051](adr/0051-feed-content-comes-from-its-source-url.md) section 4). The node always checks the crawl token first. `build_chain` panics if the list names it.

```bash
# Default (all built-ins in recommended order)
VERIFIER_CHAIN=content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type

# Add an exact-match feed blocklist
VERIFIER_CHAIN=content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type

# Skip medium_music for feeds that don't set podcast:medium yet
VERIFIER_CHAIN=content_hash,v4v_payment,enclosure_type

# Add the strict payment_route_sum check
VERIFIER_CHAIN=content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,payment_route_sum,enclosure_type
```

When `VERIFIER_CHAIN` is absent, empty, or parses to no verifier names, the
default chain is used. Unknown names in the chain are fatal startup
configuration errors; `build_chain` panics rather than silently running a
weakened gate.

The chain order matters:
- `content_hash` should be first (short-circuits unchanged feeds with no DB write)
- `feed_blocklist` should run early if you use it (rejects known-bad feeds before enrichment work)
- Remaining verifiers inspect feed content and can be reordered freely

---

## Step-by-Step: Creating a Custom Verifier

### 1. Create the verifier file

Create `src/verifiers/my_verifier.rs`:

```rust
//! Verifier: my custom check.

use crate::verify::{IngestContext, Verifier, VerifyResult};

/// Description of what this verifier does.
#[derive(Debug)]
pub struct MyVerifier;

impl Verifier for MyVerifier {
    fn name(&self) -> &'static str { "my_verifier" }

    fn verify(&self, ctx: &IngestContext) -> VerifyResult {
        let Some(feed_data) = &ctx.request.feed_data else {
            return VerifyResult::Pass; // fetch failed -- handled elsewhere
        };

        // Your validation logic here.
        if some_condition(feed_data) {
            VerifyResult::Pass
        } else {
            VerifyResult::Fail("reason for rejection".into())
        }
    }
}
```

### 2. Register the module

Add to `src/verifiers/mod.rs`:

```rust
pub mod my_verifier;
```

### 3. Add a match arm to build_chain

In `src/verify.rs`, add to the `build_chain` function:

```rust
"my_verifier" => Box::new(crate::verifiers::my_verifier::MyVerifier),
```

### 4. Configure at runtime

Set the environment variable to include your verifier:

```bash
VERIFIER_CHAIN=content_hash,my_verifier,medium_music,feed_guid,v4v_payment,enclosure_type
```

No other files need to change. The chain order and which verifiers run is controlled entirely by the environment variable -- no redeployment of other nodes is required when adding verifiers to a primary.

---

## Built-in Verifiers

### crawl_token (authentication, not a chain entry)

`crawl_token` is not a name in `VERIFIER_CHAIN`. `build_chain` panics if the list names it ([ADR 0051](adr/0051-feed-content-comes-from-its-source-url.md) section 4).

- **File:** `src/verifiers/crawl_token.rs`
- **Effect:** Rejects a submission with an incorrect crawl token
- **Result:** `Fail("invalid crawl token")` on a mismatch
- **Env vars:** None (uses the `CRAWL_TOKEN` value passed to `VerifierChain::new` at startup)
- **Notes:** Uses a constant-time compare (a SHA-256 hash through `subtle::ConstantTimeEq`). `VerifierChain::authenticate` runs this check before the node reads the database, before the configurable chain, and before the content-hash shortcut.

### content_hash

- **File:** `src/verifiers/content_hash.rs`
- **Effect:** Short-circuits unchanged feeds. If the feed's content hash matches the last crawl (stored in `feed_crawl_cache`), returns a special `Fail("NO_CHANGE")` sentinel that the ingest handler treats as a no-op rather than a rejection.
- **Result:** `Pass` if the hash is new or different; special `Fail` sentinel if unchanged
- **Env vars:** None
- **Notes:** Requires direct DB access (`feed_crawl_cache` table). The sentinel value is defined as `verifiers::content_hash::NO_CHANGE_SENTINEL`.

### medium_music

- **File:** `src/verifiers/medium_music.rs`
- **Effect:** Rejects feeds where `podcast:medium` is absent or set outside the accepted set
- **Result:** `Pass` for `music`, `publisher`, and `musicL`; `Fail` otherwise
- **Env vars:** None
- **Notes:** `publisher` and `musicL` are accepted as container/source-layer mediums. `publisher` still requires at least one `remoteItem` child with `medium="music"`. When `podcast:medium` is absent, the verifier rejects (`Fail`). Operators who want to accept feeds without the tag should remove this verifier from the chain.

### feed_blocklist

- **File:** `src/verifiers/feed_blocklist.rs`
- **Effect:** Rejects feeds whose exact GUID or URL is operator-blocked
- **Result:** `Pass` if the feed GUID, canonical URL, and source URL are all absent from the blocklists; `Fail` otherwise
- **Env vars:** `BLOCKED_FEED_GUIDS`, `BLOCKED_FEED_URLS`
- **Notes:** Exact-match only. `BLOCKED_FEED_GUIDS` is compared case-insensitively against `podcast:guid`. `BLOCKED_FEED_URLS` is compared exactly against both `canonical_url` and `source_url`, so redirects do not bypass the blocklist.

Example:

```bash
BLOCKED_FEED_GUIDS=27293ad7-c199-5047-8135-a864fb546492,27293ad7-c199-5047-8135-a864fb546491
BLOCKED_FEED_URLS=https://feeds.podcastindex.org/100retro.xml,https://feeds.podcastindex.org/100retro_test.xml
VERIFIER_CHAIN=content_hash,feed_blocklist,medium_music,feed_guid,v4v_payment,enclosure_type
```

### feed_guid

- **File:** `src/verifiers/feed_guid.rs`
- **Effect:** Rejects known-bad/placeholder `podcast:guid` values and malformed UUIDs
- **Result:** `Pass` if the GUID is a valid UUID and not in the blocklist; `Fail` otherwise
- **Env vars:** None
- **Notes:** The blocklist (`BAD_GUIDS`) contains platform-default GUIDs shared by thousands of unrelated feeds. Add new entries as they are discovered.

### v4v_payment

- **File:** `src/verifiers/v4v_payment.rs`
- **Effect:** Rejects feeds that do not participate in V4V payments
- **Result:** `Pass` if the feed has at least one valid feed-level payment route (non-empty address, positive split); `Fail` otherwise
- **Env vars:** None
- **Notes:** Validates both feed-level and track-level routes for normal music feeds. Tracks with no routes of their own are valid (they fall back to feed-level routes). Tracks that declare routes but list no valid recipients are rejected. `publisher` and `musicL` feeds are source-layer containers and are payment-exempt.

### enclosure_type

- **File:** `src/verifiers/enclosure_type.rs`
- **Effect:** Warns when any track enclosure MIME type starts with `"video/"`
- **Result:** `Warn` on video enclosures; `Pass` otherwise (never rejects)
- **Env vars:** None
- **Notes:** Video enclosures are unexpected in a music feed index but do occur (music videos). The warning is stored for audit; the feed is not rejected.

### payment_route_sum

- **File:** `src/verifiers/payment_route_sum.rs`
- **Effect:** Rejects feeds where any track's payment route splits do not sum to 100
- **Result:** `Pass` if all tracks with routes have splits summing to 100; `Fail` otherwise
- **Env vars:** None
- **Notes:** **Not in the default chain.** This is an optional strict-mode verifier. Many real-world feeds have splits that do not sum to exactly 100 (rounding, platform fees). Enable it only if your deployment requires strict split enforcement.

---

## Verifier Design Guidelines

1. **Return `Pass` when `feed_data` is `None`.** A missing `feed_data` means the crawler could not fetch/parse the feed. Other verifiers and the ingest handler deal with this; your verifier should not double-fail.

2. **Use `Fail` for hard rejections, `Warn` for soft flags.** Warnings do not block ingestion -- they are stored for audit. Use `Fail` only when the feed definitively should not be in the index.

3. **Do not write to the database.** The `db` connection in `IngestContext` is for reads only. Writes happen in the ingest handler's atomic transaction after all verifiers pass.

4. **Keep verifiers pure and fast.** Verifiers run synchronously inside a `spawn_blocking` task. Avoid network calls, heavy computation, or blocking I/O.

5. **Name your verifier with snake_case.** The name appears in warning/rejection messages as `[name] message` and is used in `VERIFIER_CHAIN`.
