# Security Audit: Feed Trust, Identity And The Fetch Tier

Date: 2026-09-24. Branch: `claude-audit`. Base commit: `85093b9`.

This record is advisory. It states no binding rule.
[ADR 0051](../adr/0051-feed-content-comes-from-its-source-url.md) to
[ADR 0055](../adr/0055-the-primary-fetches-what-it-signs.md) decide its
recommendations. [The remediation plan](../plans/feed-trust-remediation-plan.md)
gives the tasks. [The conflict analysis](feed-identity-conflict-analysis.md)
records more defects in the proof flow and in the ingest transaction.

## Scope And Method

Four agents examined the three crates. Each agent had one area:

1. The ingest trust boundary between the crawler and the node.
2. Feed identity, impersonation and takeover.
3. The crawler fetch path and the parser.
4. The signed event log, replication and the write API.

The agents read the code only. The lead then checked each high-risk claim
against the code. One claim was also checked with a temporary integration
test, which was then deleted. Each finding gives its check status:

- **Test**: a test run showed the behavior.
- **Code**: the lead read the code path.
- **Agent**: one agent reported it. The lead did not check it.

## The Reference Model

A podcastindex.org-style index uses these principles for RSS trust:

1. **The index fetches the feed.** A submitter nominates a URL. The index
   gets the content from that URL. The index does not accept content from
   the submitter.
2. **A GUID is an identity claim, not a proof.** `podcast:guid` is a
   UUIDv5 of the feed URL. Any feed can copy it. The index gives the GUID
   to the first URL that it verified, and it treats a second URL with the
   same GUID as a duplicate.
3. **Only the old location can move a feed.** A feed moves when the old URL
   gives an HTTP `301`, or when the old body contains
   `itunes:new-feed-url`. A new URL cannot take a feed by its own claim.
4. **The owner proves control in the feed.** `podcast:locked` with an
   `owner` email stops an import to a different host. A
   `podcast:txt purpose="verify"` token proves that a person controls the
   feed body.
5. **Money routes need a stable source.** A change to the value recipients
   must come from the owner location. An index can show the history of the
   recipients.
6. **Corrections are durable.** A removal stays removed. The next crawl
   does not add the removed feed again.

## The Trust Model In The Code

- The crawler parses the feed and sends an `IngestFeedData` structure to
  `POST /ingest/feed` (`src/ingest.rs:21`). The node does not fetch the
  feed and does not receive the raw body.
- One shared `CRAWL_TOKEN` authenticates each crawler. The check is
  constant-time (`src/verifiers/crawl_token.rs`).
- The ingest handler finds the existing feed by URL
  (`src/api.rs:1645`). The write goes to the row for the feed GUID
  (`src/db.rs:1320`, `ON CONFLICT(feed_guid)`).
- After the first ingest, the stored `feed_url` of a GUID does not change
  (`src/api.rs:1836`, ADR 0049 section 1). All other feed fields, the
  tracks and the payment routes come from the latest submission.
- The primary signs each event. A community node examines each signature
  before it applies the event. This part of the design is sound.

## Findings

### F1. Critical. A copied `podcast:guid` replaces the content and the payment routes of another feed

Status: **Test**.

Feed A is at `https://victim.example/feed.xml`. Feed B is at a different
URL and declares the same `podcast:guid`. The crawler fetches B. The node
accepts B. The result:

```text
FEED url=https://victim.example/feed.xml title=Attacker
TRACKS [("Attacker track", Some("attacker@ln.example"))]
FEEDROUTES ["attacker@ln.example"]
```

The API shows the victim URL with the content and the payment addresses of
the attacker. The tracks of the victim are gone. No verifier examines the
GUID against the stored URL. `feed_guid` examines only the format
(`src/verifiers/feed_guid.rs`).

Who can do this: any person who can put an RSS file on a web server and
cause a crawl. A podping from any Hive account causes a crawl in `gossip`
mode. A `podcast:remoteItem` link in a feed causes a crawl in `feed`,
`refresh` and `gossip` modes (ADR 0049 section 2). The attacker does not
need the crawl token.

A subsequent crawl of the victim URL does not repair the record.
`feed_crawl_cache` uses the submitted URL as its key (`src/api.rs:2427`).
The victim URL keeps its earlier hash. Thus an unchanged victim body gives
`NO_CHANGE`, and the content of the attacker stays. The repair needs a
forced ingest from the victim URL.

### F2. High. The index has no feed-move rule

Status: **Code**.

No crate reads `itunes:new-feed-url` or `podcast:locked`. A search of
`src/`, `stophammer-parser/src/` and `stophammer-crawler/src/` finds no
reader. The stored URL is the first URL for the GUID, and it never changes
(`src/api.rs:1836`).

This has an honest-mistake effect that is the same as F1. An artist moves
the feed to a new host, and the old host continues to serve an old copy.
The index then shows the old copy and the new copy in turn. The stored URL
stays the old host.

F1 and F2 have one root cause. The index has no owner URL for a GUID, so
it cannot decide which submission may change the content.

### F3. High. The crawler fetches internal addresses

Status: **Code**.

`is_followable_url` examines only the scheme
(`stophammer-crawler/src/follow.rs:107`). The crawler does not block
loopback, private, link-local or cloud metadata addresses. A podping URL
or a `remoteItem` URL of `http://169.254.169.254/` or
`http://localhost:8008/` is fetched.

The node already has a correct guard for the proof-of-possession fetch.
The guard does a DNS check for each address and a check on each redirect
(`src/proof.rs:883`). The crawler does not use this guard.

ADR 0006 puts this risk on deployment isolation. The isolation on the VPS
needs a check. This audit did not do that check.

### F4. High. The node trusts the parse of the crawler

Status: **Code**.

The node does not see the feed body. Thus a holder of `CRAWL_TOKEN` can
write any content for any URL and any GUID. The node signs that content
and sends it to each community node.

The token is shared and has no rotation path. An event does not record
which crawler sent it or a hash of the raw body. After a token leak, nobody can find which events are
bad. ADR 0006 states this limit.

### F5. Medium. An old copy of a feed replaces a newer copy

Status: **Code**.

`upsert_feed` replaces each field with no ordering condition
(`src/db.rs:1320`). `content_hash` compares only with the last hash
(`src/verifiers/content_hash.rs`). The node does not compare
`lastBuildDate` or the newest item date. A stale CDN copy, an old host
(F2) or a replay removes newer tracks and newer routes.

### F6. Medium. A removal does not stay removed

Status: **Code**.

`DELETE /feeds/{guid}` deletes the rows and signs a `FeedRetired` event.
Community nodes apply it (`src/apply.rs:185`). The node does not record a
block. The next podping or `remoteItem` link adds the feed again.

The only durable block is `BLOCKED_FEED_GUIDS` and `BLOCKED_FEED_URLS`
(`src/verifiers/feed_blocklist.rs`). These are environment variables. A
change needs a restart. They do not replicate to community nodes, and they
make no signed record.

### F7. Medium. The crawler reads a response body with no size limit

Status: **Code**.

`resp.bytes()` reads the full body (`stophammer-crawler/src/crawl.rs:701`
and `:872`). `fetch_timeout` limits the time, but not the size. A fast
server can send a large or compressed body and use all the memory.

### F8. Medium. One feed can cause many fetches

Status: **Agent**.

A feed can contain any number of `podcast:remoteItem` elements. The follow
waves have no limit for each source feed. One feed can cause the crawler
to send many requests to a third-party host.

### F9. Low. Configuration and contract gaps

- Any holder of the token can set `force_reingest`, which skips
  `content_hash` (`src/ingest.rs:33`). Status: **Code**.
- `VERIFIER_CHAIN` can omit `crawl_token`. Ingest then needs no token.
  Only an empty value falls back to the default (`src/verify.rs:241`).
  Status: **Code**. A probe in the conflict analysis also showed it.
- The node does not examine the scheme of an image, enclosure or link URL.
  A `javascript:` URL goes to each client. Status: **Agent**.
- Ingest rejects a body above 2 MiB and a feed above 500 tracks
  (`src/api.rs:574`, `MAX_TRACKS_PER_INGEST`). A large real feed fails, and
  the crawler must report it as a rejection. Status: **Code**.
- A community node with `ALLOW_INSECURE_PUBKEY_DISCOVERY=true` gets the
  primary key over plain HTTP (`src/community.rs:337`). Status: **Agent**.
- ADR 0005 gives a verifier order that is not the order in
  `src/verify.rs:234`. Status: **Code**.

## Parts That Are Sound

The replication agent reported these items. The lead checked the first two.

- Each event has a signature that includes `seq` (ADR 0036). A community
  node examines the signature before it applies the event.
- `FeedRetired` and `TrackRemoved` apply on community nodes
  (`src/apply.rs:185`, `:217`).
- Admin and sync tokens use a constant-time comparison.
- Peer registration is signed and uses the SSRF guard.
- The proof challenge has 128 bits of entropy, has an expiry and is single
  use.
- FTS5 input is cleaned before the query.
- `signing.key` and the TLS key have mode `0600`.
- The parser uses `roxmltree`, which does not expand external entities.

## Agent Claims The Lead Rejected

- "Ingest has no authentication." Not correct. `crawl_token` is in the
  default chain.
- "`TrackRemoved` and `FeedRetired` do not apply." Not correct. See
  `src/apply.rs:185` and `:217`.
- "The node has no body limit." Not correct. The limit is 2 MiB.
- "reqwest follows 20 redirects." The reqwest default is 10.
- "A copied GUID changes the stored `feed_url`." Not correct. The URL stays.
  The content and the routes change (F1).

## Recommendations

The first four items change the ingest contract and the storage shape.
Each one needs an ADR before code. Items 1 to 3 can be one ADR.

1. **Give each GUID one owner URL.** The first URL that gives the GUID is
   the owner. A submission from a different URL with the same GUID records
   a URL observation (ADR 0049 already does this). It does not change the
   content, the tracks or the routes. The node reports the conflict to the
   operator. This removes F1.
2. **Move a feed only by the old location.** The owner URL can move the
   feed with an HTTP `301` or with `itunes:new-feed-url` in its body. The
   proof-of-possession flow of ADR 0018 can also move it, with a
   `podcast:txt` token at both URLs. Respect `podcast:locked`. This removes
   F2 and gives an honest path for a host change.
3. **Watch the money.** Compare the value recipients with the previous
   crawl. Keep a signed history of the recipient set. Show the operator a
   change of recipient that comes with a change of source URL.
4. **Make corrections durable.** Replace the environment blocklist with a
   signed and replicated block event, and a signed unblock event.
   `DELETE /feeds/{guid}` also writes the block. Reject a submission older
   than the stored `last_build_date` or newest item date, unless an
   operator sets an override. This removes F5 and F6.
5. **Harden the crawler.** Use the `src/proof.rs` address guard for each
   fetch and each redirect hop. Set a body size limit and a redirect limit.
   Set a limit on the `remoteItem` links that the crawler follows from one
   feed. This removes F3, F7 and F8.
6. **Record provenance.** Put a crawler identity and the raw-body hash in
   each ingest event. Give each crawler its own token. Later, the node can
   fetch the feed again to examine a submission that changes routes. This
   reduces F4.
7. **Close the small gaps.** Make `crawl_token` a required verifier.
   Allow `force_reingest` only with the admin token. Accept only `http`
   and `https` in URL fields. Correct the verifier order in ADR 0005.

## Open Checks

- The network isolation of the crawler on the VPS needs a check (F3).
- The lead did not check F8 or the URL scheme claim in F9.
- The production database may already contain a GUID with content from a
  second URL. A query of `feed_url_observations` for each GUID with more
  than one URL gives the candidates.
