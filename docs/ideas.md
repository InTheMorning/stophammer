# Ideas & Future Direction

## Distributed Query API

### Problem

There is currently no query API for consumers. The only read surface is the raw
event log (`GET /sync/events`) which is designed for node replication, not lookups.
A client wanting to find all feeds by an artist has to fetch the entire event log
and filter locally — not usable in practice.

### The data is already there

The SQLite schema has `artists`, `feeds`, `tracks`, `payment_routes`, and
`value_time_splits` tables. The data exists on every node. It just has no HTTP
surface yet.

### Proposed query endpoints

| Endpoint | Returns |
|---|---|
| `GET /artists?q=heycitizens` | Artist records matching the query |
| `GET /artists/{artist_id}/feeds` | All feeds for an artist |
| `GET /feeds/{feed_guid}` | Feed + tracks + payment routes |
| `GET /feeds/{feed_guid}/tracks/{item_guid}` | Single track with value splits |
| `GET /tracks?artist_id=...` | All tracks for an artist across albums |

These would be served identically by both primary and community nodes (read-only),
backed directly by the existing SQLite tables.

### Distribution model: client-side load balancing via peer discovery

Every node holds a full copy of the index. No sharding needed. Distribution works
like this:

1. Client hits any known node's `GET /sync/peers`
2. Gets back a list of N nodes with their URLs
3. Client picks one (random, round-robin, or latency-based — client's choice)
4. Queries it directly

```
client → GET /sync/peers  (any known node)
       ← [node1_url, node2_url, node3_url, ...]
client → GET /artists?q=heycitizens  →  node2  (client chose)
```

No proxy, no coordination layer. The same model DNS resolvers use.

### What needs to change

1. **`GET /sync/peers` on community nodes** — currently primary-only. Community
   nodes should cache the peer list from the primary at startup and serve it too,
   adding themselves to the list. Any node then becomes a valid bootstrap point
   and the primary is no longer special for client traffic.

2. **The query API** — implemented once, runs on every node identically.

3. **A stable bootstrap URL** — the one thing a client needs to hardcode. After
   the first `GET /sync/peers` call the client has the full peer list and can
   reach any node without the bootstrap again. Could be the primary, the Cloudflare
   tracker, or any well-known community node.

### What you do NOT need

- Gossip query routing (Kademlia-style) — the index is small enough that full
  replication is the right model
- Consistent hashing / sharding — every node holds everything
- A load balancer in front of nodes — clients distribute themselves naturally

### Open questions

- Should `GET /sync/peers` include community nodes' self-reported load or latency
  hints so clients can make smarter choices?
- Pagination strategy for `/artists` search — simple offset or cursor-based?
- Should the query API be versioned (`/v1/artists`) from the start?
