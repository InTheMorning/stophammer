# ADR 0055: The Primary Fetches What It Signs

## Status
Proposed

## Date
2026-09-24

## Context
ADR 0006 says that the node treats each crawler submission as untrusted. The
node cannot do that today. The crawler parses the feed and sends the parsed
fields to `POST /ingest/feed` (`src/ingest.rs:21`). The node never sees the
feed body. It signs what the crawler sent.

ADR 0051 stops a feed publisher from changing a record that it does not
serve. It takes the crawler report of `source_url` as true. A holder of
`CRAWL_TOKEN` can still:

- write any title, track and payment route for any held record, with the
  stored source URL as `source_url`,
- report a redirect that did not occur, and so move a record under ADR 0052,
- admit any new feed.

The token is one shared secret. It has no rotation path. An event does not
record which crawler sent the content or a hash of the body. After a leak,
nobody can find the events that are false.

Podcast Index lets anyone nominate a URL, and the index fetches the URL
itself. The submitter gives a URL, not content.

One possible design adds a verification worker that fetches each feed again
after the crawler fetched it. That doubles the requests to each host. Wavlake holds most of the index and
answers `429` under load. ADR 0050 leaves open the question whether a `304`
counts against that limit. A design that fetches each feed twice makes that
problem worse. It also keeps the crawler parse, which nothing then uses.

## Decision

### 1. Two roles: nominator and fetch worker

| Role | Sends | Credential | Can change the index |
|---|---|---|---|
| Nominator | A URL and a reason | `NOMINATE_TOKEN` | No. It adds a URL to a queue |
| Fetch worker | The result of a job that the primary issued | `WORKER_TOKEN` | Yes, through the rules of ADR 0051, 0052 and 0053 |

The `gossip`, `import`, `feed` and `refresh` modes become nominators. They do
not fetch feed bodies. The podping filters, the import cursor and the node
feed list of ADR 0047 stay in those modes.

The fetch worker is a new mode of `stophammer-crawler`. It is the only
process that fetches a feed body for ingest. It keeps the fetch cache of
ADR 0050, the per-host delay and the fetch rules of ADR 0054. It parses with
`stophammer-parser`, so the parser stays outside the primary binary under
ADR 0017.

Each feed is fetched once for each crawl, as today.

ADR 0056 removed the ADR 0018 proof fetch, so no public request makes the
primary fetch a URL.

### 2. The primary owns the queue

- `POST /v1/nominations` with `NOMINATE_TOKEN` adds a URL. The primary
  ignores a URL that the queue already holds. It keeps at most one pending
  job for each URL, and limits the queue for each host and in total.
- The primary makes a job from a nomination, or from its own schedule. A job
  has an ID, the URL, a purpose, the record and its source URL when a record
  exists, and an expiry time.
- The worker takes jobs from `GET /internal/jobs` and returns each result to
  `POST /internal/jobs/{id}/result`. Both routes need `WORKER_TOKEN`. They
  listen only on an address that the operator configures for the worker, not
  on the public address.

### 3. The result of a job

A result holds:

- the job ID,
- each redirect hop with its URL and status, and the final URL,
- the HTTP status, and the validators of ADR 0050,
- the SHA-256 of the body,
- the parser version,
- the parsed fields.

The primary accepts a result only for an open job, once, before the expiry.
It applies the result with the rules of ADR 0051, 0052 and 0053. The URL of
the job is `source_url`. The hops come from the worker, not from a
nominator.

The primary rejects a result for a record whose source URL changed after the
job was made. Thus a slow result cannot undo a move.

### 4. Each accepted fetch has a signed record

The primary emits a new signed event, `FeedFetched`, in the same transaction
as the content. It holds the job ID, the URL, the final URL, the SHA-256 of
the body, the parser version and the worker ID. A later audit can find each
event that one worker or one body produced.

### 5. The nominators discover links through the primary

A result also gives the follow URLs of ADR 0049 section 2 and the
`itunes:new-feed-url` of ADR 0052. The primary adds them to the queue under
the limits of ADR 0054 section 3. The follow waves thus run in the primary
queue, not in a crawler.

### 6. What each credential can do after a leak

| Leaked credential | Effect |
|---|---|
| `NOMINATE_TOKEN` | The queue gets more URLs. The limits of section 2 stop a flood. No content changes |
| `WORKER_TOKEN` | Results for open jobs only. The attacker must also reach the private address |
| The worker host | The same as a compromise of the primary. The worker has no database access and no signing key, but its results are trusted |

### 7. Transition

1. Add the queue, the job routes and the worker mode. The worker and the old
   ingest path run together. `POST /ingest/feed` stays open with
   `CRAWL_TOKEN`.
2. Move `refresh`, then `feed`, then `import`, then `gossip` to nominations.
   Compare the result of each mode with the old path for one pass.
3. Remove `POST /ingest/feed` and `CRAWL_TOKEN`. From this step, no content
   reaches the primary except as a job result.

`FeedFetched` is a new event type. The rollout upgrades each community node
before the primary emits one.

## Alternatives Considered

### Fetch each feed again after the crawler
It gives the same trust result with twice the requests to each host, and it
keeps a parse that nothing uses.
Rejected.

### The primary fetches in its own process
This puts the SSRF risk and the XML parser in the process that holds the
signing key. ADR 0017 keeps the parser outside the primary binary. Rejected.

### Sign each crawler report with a key for each crawler
A signature proves which crawler made a statement. It does not prove that
the statement is true. It helps an audit after a leak, and section 4 gives
that. Rejected as the trust control.

## Consequences

- A leak of the nominator credential cannot change content.
- The number of requests to each feed host does not increase.
- Discovery modes can run on a host that the operator does not control,
  because they submit only URLs.
- The worker and the primary are one trust boundary. A compromise of the
  worker host is a compromise of the index.
- The crawler crate changes more than in any earlier ADR. Four modes lose
  their fetch path, and one mode is new.
- Plain `http` feeds stay open to a change on the network. A compromise of a
  feed host stays outside this model.

## Invariants

- The primary applies content only from a result of a job that it issued.
- A nominator credential never changes content.
- Each accepted fetch has a signed `FeedFetched` event with the SHA-256 of
  the body.

## Guards

No leak of `CRAWL_TOKEN` is known. The guards come from the design risk:

- A result for an unknown, expired or used job ID changes nothing.
- A result for a record whose source URL changed after the job changes
  nothing.
- A nomination with parsed fields is rejected. The route takes a URL only.
- The job routes answer only on the configured worker address.
