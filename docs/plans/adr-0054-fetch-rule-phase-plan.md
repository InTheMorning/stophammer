# ADR 0054 Phase Plan: The Fetch Rule

Owner: [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md). This
plan states no rule.

## Goal

A fetch of a URL from RSS or from a podping reaches only public addresses. It
reads at most 16 MiB, and it follows at most 10 redirects. One source feed
cannot cause an unlimited number of fetches. No read route serves a non-web
URL.

## Non-Goals

- A shared crate for the guard. ADR 0054 section 5 rejects it.
- ADR 0055.
- Network isolation of the containers. The operator does that outside the
  code.

## Assumptions

- The crawler follows redirects by hand in `fetch_following_redirects`
  (`stophammer-crawler/src/crawl.rs`, ADR 0052 task 005).
- The crawler reads each body with `resp.bytes()`. `reqwest` has the `stream`
  feature and no decompression feature in either crate. Thus the bytes that
  the crawler reads are the decoded body.
- `src/fetch_guard.rs` in the node has `is_private_ip`. It misses multicast,
  the documentation ranges, the benchmark range, the IPv4-mapped and
  IPv4-compatible IPv6 forms, and NAT64.
- `is_followable_url` in `stophammer-crawler/src/follow.rs` examines only the
  scheme.

## Decisions For The Tasks

1. **One address test in each crate.** `is_public_ip(IpAddr) -> bool`. It is
   true only for a public unicast address, as ADR 0054 section 1 lists. An
   IPv4-mapped, IPv4-compatible or NAT64 (`64:ff9b::/96`) IPv6 address takes
   the result of the IPv4 address inside it.
2. **The crawler pin.** Each feed fetch client gets a custom DNS resolver
   (`reqwest::dns::Resolve`). The resolver resolves with the system resolver,
   and fails when any address is not public. The connection uses only the
   addresses that the resolver returned, so no second answer can change the
   target.
3. **Literal and URL checks.** Before each request of the redirect loop, the
   crawler rejects three things:
   - a scheme other than `http` or `https`,
   - a user name or a password,
   - a host that is an IP literal and not public. The resolver does not see
     an IP literal.
4. **The reason.** A rejected target is a final fetch error with the reason
   `fetch_target_not_public: <host>`. It is not retryable.
5. **The body limit.** `MAX_FEED_BODY_BYTES = 16 * 1024 * 1024`. The crawler
   reads the body with `chunk()` and stops at the limit, with the final fetch
   error `body_too_large`. A `Content-Length` over the limit fails before the
   first chunk.
6. **The follow limits.** At most 200 follow URLs from one source feed in one
   wave, and at most 50,000 URLs in the follow queue of one pass. The crawler
   logs the number that it drops, with the source URL.
7. **The node guard.** `is_private_ip` in `src/fetch_guard.rs` becomes the
   negation of `is_public_ip` of decision 1. Its callers do not change.
8. **The cases.** Each crate has one test with the cases of ADR 0054 section
   5. The resolver case, a public name that resolves to `127.0.0.1`, uses a
   test resolver.
9. **The URL fields.** The node keeps each raw value. At ingest it adds the
   warning `non-web URL in <field>` for each URL field from RSS whose scheme
   is not `http` or `https`. Each read route passes each such field through
   one function, `web_url_or_none`, which returns `None` for a non-web value.

## Sequence

| Task | Repository | Needs |
|---|---|---|
| [001](../tasks/adr-0054-task-001-crawler-address-guard.md) Crawler address guard | `stophammer-crawler` | — |
| [002](../tasks/adr-0054-task-002-crawler-limits.md) Crawler body and follow limits | `stophammer-crawler` | 001 |
| [003](../tasks/adr-0054-task-003-node-guard-and-url-fields.md) Node guard and URL fields | `stophammer` | — |
| [004](../tasks/adr-0054-task-004-deploy.md) Deploy | VPS | 001 to 003 |

Task 003 can run beside task 001, because they change different
repositories.

## Schema And API Implications

- No migration and no event type.
- A read route can return `null` for a URL field that had a value before.
- The ingest response can carry a new warning.

## Risk Areas

- **A feed host behind a private address on purpose.** None is known. The
  crawler logs each rejection with the host, so the operator sees one.
- **The resolver and the connection pool.** A pooled connection to a host
  does not resolve again. That is correct, because the first resolution was
  checked.
- **A read route that builds a URL field in more than one place.** Task 003
  lists each field and each route.

## Test Strategy

- The cases of ADR 0054 section 5, in each crate.
- A stub server that sends a body over the limit, with and without
  `Content-Length`.
- A feed with 201 follow URLs.
- A read route test for each URL field with a `javascript:` value.

## Rollback

Deploy the previous images. No stored data changes.
