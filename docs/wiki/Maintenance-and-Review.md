# Maintenance and Review

## Why These Tools Exist

`stophammer` now keeps richer source evidence and a separate canonical layer.
That means operators sometimes need to rebuild derived state or inspect why a
merge happened.

The shipped maintenance binaries are:

- `resolverd`
- `resolverctl`
- `backfill_canonical`
- `backfill_artist_identity`
- `review_artist_identity`

## Typical Maintenance Flow

### Keep the durable canonical resolver queue drained

```bash
cargo run --bin resolverd
```

For large imports, pause background draining first:

```bash
cargo run --bin resolverctl -- import-active
# run import
cargo run --bin resolverctl -- import-idle
```

### Rebuild canonical rows after schema or resolver changes

```bash
cargo run --bin backfill_canonical -- --db ./stophammer.db
```

### Re-run deterministic artist identity backfill

```bash
cargo run --bin backfill_artist_identity -- --db ./stophammer.db
```

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

- [operations.md](/home/citizen/build/stophammer/docs/operations.md)
- [resolver-refactor-plan.md](/home/citizen/build/stophammer/docs/resolver-refactor-plan.md)
- [schema-reference.md](/home/citizen/build/stophammer/docs/schema-reference.md)
- [review_artist_identity.1](/home/citizen/build/stophammer/man/review_artist_identity.1)
