# Verifier Development Guide

This guide explains how the verifier chain works and how to create custom verifiers for the Stophammer primary node.

---

## What the Verifier Chain Is

The verifier chain is an ordered pipeline of validation steps that every `POST /ingest/feed` request passes through on the primary node. Each verifier inspects the incoming feed data and decides whether to pass, warn, or reject.

The chain is the core quality gate: it is the reason Stophammer's index only contains verified V4V music feeds. Community nodes do NOT run verifiers -- they verify the ed25519 signature on events and trust the primary's verification result. Verifier warnings from the primary are stored in each event's `warnings` field as an audit trail replicated to all nodes.

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
| `raw_medium` | `Option<String>` | `podcast:medium` tag value (e.g. `"music"`) |
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

The `VERIFIER_CHAIN` environment variable controls which verifiers run and in what order. It is a comma-separated list of verifier names:

```bash
# Default (all built-ins in recommended order)
VERIFIER_CHAIN=crawl_token,content_hash,medium_music,feed_guid,v4v_payment,enclosure_type

# Skip medium_music for feeds that don't set podcast:medium yet
VERIFIER_CHAIN=crawl_token,content_hash,v4v_payment,enclosure_type

# Add the strict payment_route_sum check
VERIFIER_CHAIN=crawl_token,content_hash,medium_music,feed_guid,v4v_payment,payment_route_sum,enclosure_type
```

When `VERIFIER_CHAIN` is absent or empty, the default chain is used. Unknown names in the chain are logged as warnings and skipped -- they do not abort startup.

The chain order matters:
- `crawl_token` should always be first (rejects unauthenticated requests before any DB access)
- `content_hash` should be second (short-circuits unchanged feeds with no DB write)
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
VERIFIER_CHAIN=crawl_token,content_hash,my_verifier,medium_music,feed_guid,v4v_payment,enclosure_type
```

No other files need to change. The chain order and which verifiers run is controlled entirely by the environment variable -- no redeployment of other nodes is required when adding verifiers to a primary.

---

## Built-in Verifiers

### crawl_token

- **File:** `src/verifiers/crawl_token.rs`
- **Effect:** Rejects requests with an invalid crawl token
- **Result:** `Fail("invalid crawl token")` on mismatch
- **Env vars:** None (uses the `CRAWL_TOKEN` passed to `build_chain` at startup)
- **Notes:** Uses constant-time comparison (SHA-256 hash via `subtle::ConstantTimeEq`). Should always be first in the chain to gate all other checks.

### content_hash

- **File:** `src/verifiers/content_hash.rs`
- **Effect:** Short-circuits unchanged feeds. If the feed's content hash matches the last crawl (stored in `feed_crawl_cache`), returns a special `Fail("NO_CHANGE")` sentinel that the ingest handler treats as a no-op rather than a rejection.
- **Result:** `Pass` if the hash is new or different; special `Fail` sentinel if unchanged
- **Env vars:** None
- **Notes:** Requires direct DB access (`feed_crawl_cache` table). The sentinel value is defined as `verifiers::content_hash::NO_CHANGE_SENTINEL`.

### medium_music

- **File:** `src/verifiers/medium_music.rs`
- **Effect:** Rejects feeds where `podcast:medium` is absent or set to a non-music value
- **Result:** `Pass` if `raw_medium == "music"`; `Fail` otherwise
- **Env vars:** None
- **Notes:** When `podcast:medium` is absent, the verifier rejects (`Fail`). Operators who want to accept feeds without the tag should remove this verifier from the chain.

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
- **Notes:** Validates both feed-level and track-level routes. Tracks with no routes of their own are valid (they fall back to feed-level routes). Tracks that declare routes but list no valid recipients are rejected.

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
