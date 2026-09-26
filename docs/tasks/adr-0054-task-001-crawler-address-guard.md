# ADR 0054 Task 001: Crawler Address Guard

Owner: [ADR 0054](../adr/0054-a-fetch-reaches-only-public-feed-hosts.md)
sections 1 and 5. Plan: [phase plan](../plans/adr-0054-fetch-rule-phase-plan.md),
decisions 1 to 4 and 8.

Repository: `stophammer-crawler`. Read `stophammer-crawler/AGENTS.md` first.

## Goal

Each feed fetch of the crawler connects only to a public address, on the first
URL and on each redirect hop.

## Files To Inspect

- `stophammer-crawler/AGENTS.md`
- `stophammer-crawler/src/crawl.rs`: `fetch_following_redirects`,
  `build_hop_request`, the fetch error outcomes
- `stophammer-crawler/src/modes/batch.rs`: `build_feed_fetch_client`
- `stophammer-crawler/src/modes/gossip.rs`: `create_async_client`,
  `create_sse_client`
- `stophammer-crawler/src/modes/import.rs`: its fetch client
- `stophammer-crawler/src/follow.rs`: `is_followable_url`
- `src/fetch_guard.rs` in the `stophammer` node, as a reference only

## Files Likely To Change

- `stophammer-crawler/src/fetch_guard.rs`, new
- `stophammer-crawler/src/main.rs` or `src/lib.rs`: the module line
- `stophammer-crawler/src/crawl.rs`, `src/modes/batch.rs`,
  `src/modes/gossip.rs`, `src/modes/import.rs`

## Do Not Touch

- `stophammer`, `stophammer-parser`
- The SSE client of `gossip`. It connects to the local listener on purpose.
- The ingest client. It connects to the node on purpose.

## Constraints

- **`src/fetch_guard.rs`**, with a `//!` doc that names stophammer ADR 0054:
  - `pub fn is_public_ip(ip: IpAddr) -> bool`. False for each range of ADR
    0054 section 1: IPv4 loopback, private, link-local, CGNAT
    (`100.64.0.0/10`), multicast, broadcast, unspecified, documentation
    (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`) and benchmark
    (`198.18.0.0/15`). IPv6 loopback, unspecified, ULA (`fc00::/7`),
    link-local (`fe80::/10`), multicast (`ff00::/8`) and documentation
    (`2001:db8::/32`). An IPv4-mapped (`::ffff:0:0/96`), IPv4-compatible
    (`::/96`) or NAT64 (`64:ff9b::/96`) address takes the result of the IPv4
    address inside it.
  - `pub struct PublicOnlyResolver`, which implements `reqwest::dns::Resolve`.
    It resolves the name with `tokio::net::lookup_host`. It fails with an
    error that names the host when the answer is empty or holds one address
    that is not public.
  - `pub fn check_target(url: &Url) -> Result<(), String>`. It rejects a
    scheme other than `http` or `https`. It rejects a URL with a user name or
    a password. It rejects a host that is an IP literal and not public.
- **The clients.** Each feed fetch client gets
  `.dns_resolver(Arc::new(PublicOnlyResolver))`: the batch client, the gossip
  feed client and the import client.
- **The loop.** `fetch_following_redirects` calls `check_target` on the first
  URL and on each redirect target, before the request.
- **The outcome.** A rejected target, from `check_target` or from the
  resolver, is a `FetchError` with `retryable: false` and a reason that starts
  with `fetch_target_not_public`. Find the resolver error in the `reqwest`
  error chain.
- `is_followable_url` also calls `check_target` on the parsed URL. A follow
  URL that fails is not queued.
- A comment `// CRIT-0X` is not needed. A short comment names ADR 0054.

## Implementation Steps

1. Add `src/fetch_guard.rs`.
2. Set the resolver on each feed fetch client.
3. Call `check_target` in the loop and in `is_followable_url`.
4. Add the tests below.
5. Run the gate.

## Acceptance Criteria

Mechanical:

- A unit test with each case of ADR 0054 section 5 for `is_public_ip` and
  `check_target`.
- `is_public_ip` is false for `::ffff:10.0.0.1`, `::127.0.0.1` and
  `64:ff9b::7f00:1`, and true for `::ffff:1.1.1.1`.
- A test with a resolver stub, or a test of the check that the resolver
  uses, shows that a name that resolves to `127.0.0.1` fails.
- A stub server on `127.0.0.1` answers `301` to itself: the fetch of the
  first URL fails with `fetch_target_not_public`. The crawler tests that use
  a local stub server need a test switch for this rule. Add
  `CrawlConfig::allow_private_targets`, `false` by default, set only in
  tests, and name it in the report.
- The gate is green.

## Test Commands

```bash
cd stophammer-crawler
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Escalation Triggers

Stop and report when:

- A test switch for private targets reaches a production code path, for
  example through an environment variable.
- `reqwest` 0.12 does not use the custom resolver for a redirect hop that
  the loop requests.

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `docs/tasks/adr-0054-task-001-crawler-address-guard.md`, all of it
- `docs/adr/0054-a-fetch-reaches-only-public-feed-hosts.md`
- `docs/plans/adr-0054-fetch-rule-phase-plan.md`, decisions 1 to 4 and 8
- The files in "Files To Inspect"

Goal:
- Each feed fetch of the crawler connects only to a public address.

Constraints:
- Follow "Constraints" of the task file exactly.
- Do not run any git command that writes. Do not commit.

Do not touch:
- `stophammer`, `stophammer-parser`, the SSE client, the ingest client

Acceptance criteria:
- The tests of "Acceptance Criteria".
- The gate is green.

Test commands:
- The commands of "Test Commands".

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
