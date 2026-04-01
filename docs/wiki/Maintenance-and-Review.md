# Maintenance and Review

## Why These Tools Exist

`stophammer` now keeps richer source evidence and a separate canonical layer.
That means operators sometimes need to rebuild derived state or inspect why a
merge happened.

The shipped maintenance binaries are:

- `stophammer-resolverd`
- `stophammer-resolverctl`
- `backfill_canonical`
- `backfill_artist_identity`
- `review_artist_identity`
- `review_artist_identity_tui`
- `backfill_wallets`
- `review_wallet_identity`
- `review_wallet_identity_tui`
- `review_source_claims_tui`

## Typical Maintenance Flow

### Keep the durable canonical resolver queue drained

```bash
cargo run --bin stophammer-resolverd
```

Run `stophammer-resolverd` on the primary only. Community nodes now apply the primary's
signed source-read-model, canonical, promotion, and artist-identity resolver
events instead of running local resolver batches.

For large imports, pause background draining first:

```bash
cargo run --bin stophammer-resolverctl -- import-active
# run import
cargo run --bin stophammer-resolverctl -- import-idle
```

When crawler import mode runs with `RESOLVER_DB_PATH=/path/to/stophammer.db`,
it performs this bracketing automatically and refreshes the import heartbeat.
`stophammer-resolverd` will ignore stale heartbeats and resume work if an importer dies
without clearing the pause state.

To inspect whether canonical views are caught up:

```bash
curl http://127.0.0.1:8008/v1/resolver/status
```

That endpoint reports queue backlog, import/backfill pause heartbeat state,
and which HTTP endpoints are immediate source-layer reads versus
resolver-backed.

### Rebuild canonical rows after schema or resolver changes

```bash
cargo run --bin backfill_canonical -- --db ./stophammer.db
```

This automatically coordinates with `stophammer-resolverd` via
`resolver_state.backfill_active`.

### Re-run deterministic artist identity backfill

```bash
cargo run --bin backfill_artist_identity -- --db ./stophammer.db
```

This automatically coordinates with `stophammer-resolverd` via
`resolver_state.backfill_active`.

### Review unresolved duplicate artist-name groups

```bash
cargo run --bin review_artist_identity -- --db ./stophammer.db --limit 20
```

Or inspect one name:

```bash
cargo run --bin review_artist_identity -- --db ./stophammer.db --name haleen --json
```

Or inspect the targeted resolver plan for one feed:

```bash
cargo run --bin review_artist_identity -- --db ./stophammer.db --feed-guid feed-guid-here
```

Or list feeds whose targeted resolver plan still has candidate groups:

```bash
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-feeds --limit 20
```

Or work from the stored review queue:

```bash
# List pending review items
cargo run --bin review_artist_identity -- --db ./stophammer.db --pending-reviews --limit 20

# Inspect one review item
cargo run --bin review_artist_identity -- --db ./stophammer.db --show-review 17

# Store a merge override
cargo run --bin review_artist_identity -- --db ./stophammer.db \
  --merge-review 17 --target-artist artist-123 --note "same artist, operator confirmed"

# Store a do-not-merge override
cargo run --bin review_artist_identity -- --db ./stophammer.db \
  --reject-review 17 --note "different projects sharing one name"
```

Stored artist review rows now include:

- `confidence`
- `explanation`
- `supporting_sources` for scored sources such as `likely_same_artist`

Or use the interactive review console:

```bash
cargo run --bin review_artist_identity_tui -- --db ./stophammer.db --limit 200
```

### Rebuild wallet identity and inspect pending wallet reviews

```bash
# Rebuild wallet endpoints, classifications, and artist links
cargo run --bin backfill_wallets -- --db ./stophammer.db

# Re-derive wallet display names and regenerate review items
cargo run --bin backfill_wallets -- --db ./stophammer.db --refresh

# Review pending wallet identity items
cargo run --bin review_wallet_identity -- --db ./stophammer.db
cargo run --bin review_wallet_identity -- --db ./stophammer.db --show-review 42
cargo run --bin review_wallet_identity -- --db ./stophammer.db --show-wallet wallet-id-here

# Interactive wallet review
cargo run --bin review_wallet_identity_tui -- --db ./stophammer.db --limit 200
```

Wallet review rows now include:

- review `confidence`
- `explanation`
- `supporting_sources` for scored sources such as `likely_wallet_owner_match`

These are separate from the wallet's own `class_confidence`.

### Inspect source claims and resolved promotions interactively

```bash
cargo run --bin review_source_claims_tui -- --db ./stophammer.db --limit 200
```

Useful keys once inside:

- `o` queue overview
- `p` backlog playbook
- `s` selected-feed summary
- `t` selected-track claim mix
- `h` source-claim hotspots
- `c` selected-feed conflicts
- `m` selected-feed claim mix
- `n` / `N` next / previous feed with the same dominant claim family
- `[` / `]` previous / next track with the same dominant claim family

## Resolution Inspection via API

You can also inspect why canonical mappings happened through HTTP:

- `/v1/artists/{id}/resolution`
- `/v1/releases/{id}/resolution`
- `/v1/recordings/{id}/resolution`

These routes expose stored evidence such as:

- source IDs
- source links
- platform claims
- match types
- confidence values

## Related Docs

- [operations.md](../operations.md)
- [resolver-refactor-plan.md](../resolver-refactor-plan.md)
- [schema-reference.md](../schema-reference.md)
- [review_artist_identity.1](../../man/review_artist_identity.1)
- [review_artist_identity_tui.1](../../man/review_artist_identity_tui.1)
- [backfill_wallets.1](../../man/backfill_wallets.1)
- [review_wallet_identity.1](../../man/review_wallet_identity.1)
- [review_wallet_identity_tui.1](../../man/review_wallet_identity_tui.1)
- [review_source_claims_tui.1](../../man/review_source_claims_tui.1)
