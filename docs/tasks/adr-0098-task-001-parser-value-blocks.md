# ADR 0098 Task 001: The Parser Reads Each Value Block

Owner: [ADR 0098](../adr/0098-a-value-block-keeps-its-type-and-method.md) §1,
§2 and §3.

Repository: `stophammer-parser`, in the folder `stophammer-parser/`. This task
has no dependency on the other tasks of ADR 0098.

## Goal

The parser gives each payment route and each value time split with the type,
the method and the index of its `podcast:value` block. A recipient keeps the
`type` text of the feed.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Recipient reader | extend | `stophammer-parser/src/engine.rs` | `read_payment_recipient` serves the routes and the local splits |
| Route and split types | extend | `stophammer-parser/src/types.rs` | `RouteType`, `IngestPaymentRoute` and `IngestValueTimeSplit` get the new data |

## Files To Inspect

- `stophammer-parser/AGENTS.md`
- `stophammer-parser/src/types.rs`: `RouteType`, `IngestPaymentRoute`,
  `IngestValueTimeSplit`
- `stophammer-parser/src/engine.rs`: `read_payment_recipient`,
  `extract_payment_routes`, `extract_value_time_splits`, and their callers in
  `FeedParser::parse`, `parse_item` and `parse_live_item`
- `stophammer-parser/tests/payment.rs`, `tests/regression.rs`,
  `tests/value_time_split.rs` and `tests/fixtures/payment.xml`

## Files Likely To Change

- `stophammer-parser/src/types.rs`
- `stophammer-parser/src/engine.rs`
- `stophammer-parser/tests/payment.rs`
- `stophammer-parser/tests/regression.rs`
- `stophammer-parser/tests/value_time_split.rs`
- New: `stophammer-parser/tests/fixtures/value_blocks.xml`

## Do Not Touch

- Each file outside `stophammer-parser/`.
- `Cargo.toml` and the version number.
- The rules of a value time split in ADR 0073 §1. Only the new fields change.

## Constraints

- `RouteType` keeps `Node` and `Lnaddress` and gets `Other(String)`. With the
  `serde` feature it writes a plain JSON string: `"node"`, `"lnaddress"`, or the
  text of `Other`. It reads `"node"` and `"lnaddress"` into the known values
  and each other string into `Other`. Do not use a derived form that writes an
  object.
- A recipient `type` is trimmed and in lowercase. A missing or empty `type`
  gives `Node`. `lnaddress` gives `Lnaddress`. `node` gives `Node`. Each other
  text gives `Other` with that text. Thus `type="lightning"` gives
  `Other("lightning")` and `type="address"` gives `Other("address")`.
- `IngestPaymentRoute` and `IngestValueTimeSplit` get three public fields:
  - `value_type: String`: the `type` attribute of the block, trimmed and in
    lowercase. A missing or empty attribute gives `lightning`.
  - `value_method: Option<String>`: the `method` attribute, trimmed and in
    lowercase. A missing or empty attribute gives `None`.
  - `block_index: u32`: the position of the block in the `podcast:value`
    blocks of the same channel, item or live item, from 0. A block with no
    recipient and no split also uses one position.
- Each recipient of a local value time split is an `IngestPaymentRoute` and
  gets the three values of the block that holds the split.
- The parser keeps a block of each type. Recipient order in a block stays the
  source order, and the blocks stay in source order.
- Lints: `[lints.clippy] pedantic = "deny"`.
- Write each comment in Simplified Technical English. Read
  `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Steps

1. Change `RouteType` and add its serde code.
2. Add the three fields to the two structs.
3. Read the block attributes in `extract_payment_routes` and
   `extract_value_time_splits`, and give them to `read_payment_recipient`.
4. Change `unknown_route_type_defaults_to_node` in `tests/regression.rs` to
   the new rule, with a new name.
5. Add the fixture and the tests of the acceptance criteria.

## Acceptance Criteria (mechanical)

Each criterion is a test in `stophammer-parser/tests/`.

1. A channel with a Lightning block and a block with `type="usdc"` and
   `method="eip3009"` gives routes with `block_index` 0 and 1, the value types
   `lightning` and `usdc`, and the methods `None` and `Some("eip3009")`.
2. A recipient with `type="address"` gives `RouteType::Other("address")`. Its
   address is the full `eip155:` text.
3. A block with no `type` gives `lightning`. `type=" USDC "` gives `usdc`.
4. An item with a value time split in a block with `type="usdc"` gives a split
   with the value type `usdc`, and each local recipient of that split has the
   same three values.
5. With the `serde` feature, `RouteType::Other("address")` writes `"address"`
   and reads back as the same value. `"node"` reads as `Node`.
6. A recipient with no `type` gives `Node`, as before.

## Checks

Run these commands in `stophammer-parser/`:

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

Also run `cargo build` in `stophammer-crawler/`, because that crate builds
against this crate by path. If that build fails, report the error and do not
change the crawler.

## Report

Give the changed files, the result of each check and each point where the code
did not let you follow this packet.
