# ADR 0054: A Fetch Reaches Only Public Feed Hosts

## Status
Accepted on 2026-09-25

## Date
2026-09-24

## Context
The crawler fetches URLs that strangers supply. A URL comes from a podping,
from a `podcast:remoteItem` in a feed (ADR 0049 section 2), from the
PodcastIndex snapshot, and from each redirect. ADR 0052 adds
`itunes:new-feed-url`.

ADR 0006 calls the crawler "the system's SSRF-exposed fetch tier". It puts
the risk on deployment isolation, and it asks for fetch hardening. The code
has no hardening:

- `is_followable_url` examines only the scheme
  (`stophammer-crawler/src/follow.rs:107`). A URL to `localhost`, to a
  private address or to `169.254.169.254` is fetched.
- `resp.bytes()` reads the full body with no size limit
  (`stophammer-crawler/src/crawl.rs:701` and `:872`). `fetch_timeout` limits
  the time, not the size.
- The crawler follows at most 10 redirects and records each hop (ADR 0052
  task 005). It examines no hop against a rule of address.
- A feed can hold any number of `podcast:remoteItem` elements. The follow
  waves have no limit for each source feed.

The node already has a correct guard for the fetch of sync registration
(`src/proof.rs:883`, which ADR 0056 moves to `src/fetch_guard.rs`). It rejects each private and reserved range, resolves
DNS and examines each address, pins the address for the connection, and
examines each redirect hop. The crawler does not depend on the `stophammer`
crate, so it cannot call that guard.

The node also passes URL fields from RSS to each client with no check of the
scheme. An image URL of `javascript:alert(1)` reaches a client that can
render it.

## Decision

### 1. The rule for a fetch target

This ADR owns one rule for each process that fetches a URL from RSS or from
a podping. Today these are the crawler, sync registration and the relocation
check. ADR 0055 adds the verification worker.

A fetch is permitted only when all of these are true:

- The scheme is `http` or `https`.
- The URL has no user name and no password.
- Each address that DNS gives for the host is a public unicast address. The
  fetch rejects loopback, private, link-local, CGNAT, multicast, unspecified,
  documentation and benchmark ranges for IPv4. It rejects loopback, ULA,
  link-local, multicast, unspecified, documentation, IPv4-mapped and
  IPv4-compatible forms of a rejected IPv4 address, and NAT64 forms for IPv6.
- The connection goes to an address that the check examined. The client pins
  the resolved address, so a second DNS answer cannot change the target.

The fetch applies the rule to the first URL and to each redirect hop. The
fetch follows at most 10 redirects.

### 2. Limits for one fetch

| Limit | Value |
|---|---|
| Decoded body | 16 MiB. The fetch reads the body as a stream and stops at the limit |
| Redirects | 10 |
| Total time | `fetch_timeout`, as today |

A body over the limit is a final fetch error with the reason `body_too_large`.
The limit applies after decompression, so a small compressed body cannot
expand past it.

### 3. Limits for following links

- The crawler follows at most 200 URLs from one source feed in one wave.
  It reports the number that it did not follow.
- The follow queue of one pass holds at most 50,000 URLs. The crawler
  reports each URL that it drops.
- The per-host delay and the podping cooldowns of `stophammer-crawler`
  continue to apply.

### 4. The node serves only web URLs in URL fields

The node examines each URL field from RSS at ingest: feed and track images,
enclosures, links and `podcast:remoteItem` URLs. It keeps the raw value, as
mandate 3 of `AGENTS.md` requires for source data. It adds a warning to the
ingest response for a value with a scheme other than `http` or `https`. The
rest of the submission applies.

Each read route returns `null` for such a value. No client gets a
`javascript:`, `data:` or other non-web URL from the index. The raw value
stays in the database for the operator.

### 5. One rule, two implementations, one list of cases

The node and the crawler are separate crates. Each one implements section 1.
This ADR gives the cases that both must pass. A shared crate is not made for
one function.

| Input | Result |
|---|---|
| `http://127.0.0.1/`, `http://[::1]/`, `http://localhost/` | Rejected |
| `http://10.0.0.1/`, `http://172.16.0.1/`, `http://192.168.0.1/` | Rejected |
| `http://169.254.169.254/` | Rejected |
| `http://100.64.0.1/` | Rejected |
| `http://[fc00::1]/`, `http://[fe80::1]/` | Rejected |
| `http://[::ffff:127.0.0.1]/`, `http://[64:ff9b::a00:1]/` | Rejected |
| A public host whose DNS name resolves to `127.0.0.1` | Rejected |
| A public URL that redirects to `http://127.0.0.1/` | Rejected at the hop |
| `ftp://example.com/feed.xml`, `file:///etc/passwd` | Rejected |
| `http://user:pass@example.com/` | Rejected |
| A public `https` URL | Permitted |

## Alternatives Considered

### Isolation of the network only
ADR 0006 asks for it, and the operator should also do it. A firewall rule on
one VPS is not in the code, and no test finds its loss. Both are necessary.
Rejected as the only control.

### Remove a non-web URL at ingest
This loses the raw value, against mandate 3. The read routes give the same
protection to a client. Rejected on 2026-09-25.

### Five redirects
The crawler follows 10, the reqwest default, with no known loop. A lower
limit can break a working feed and gives no additional protection, because
the rule of section 1 examines each hop. Rejected on 2026-09-25.

### A shared crate for the guard
It adds a fourth crate and a release sequence for one function. The shared
list of cases gives the same result. Rejected.

## Consequences

- A podping or a feed cannot make the crawler fetch an internal service.
- One hostile body cannot use all the memory of the crawler.
- One feed cannot cause an unlimited number of fetches.
- A feed with a body over 16 MiB fails. On 2026-09-25 the largest body of the
  10,202 held feeds in the fetch cache was 0.25 MiB. The large bodies in the
  cache were podcast feeds of 21 to 47 MiB, which the node does not hold.
- A client gets no `javascript:` or `data:` URL from the index. The raw value
  stays in the database.

## Invariants

- No fetch of a URL from RSS or from a podping connects to a non-public
  address.
- No fetch reads more than the body limit.

## Guards

The rule has no incident. The code has the gap, and the node fetch had the
same class of defect before the guard of `src/proof.rs:883`. Each crate runs the cases of
section 5 as a test. A failure message names ADR 0054 section 1.
