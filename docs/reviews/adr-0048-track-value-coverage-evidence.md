# ADR 0048 Evidence: Track-Level Value Coverage

## Purpose

[ADR 0048](../adr/0048-every-track-resolves-to-a-payment-route.md) states that
the measurement belongs to its acceptance and not to its statement. This record
holds that measurement.

The primary question is if the feeds that `V4VPaymentVerifier` refuses carry
`podcast:value` blocks on their items. If they do, the present rule refuses
feeds that pay each track. If they do not, the present rule is correct, and
ADR 0048 must be rejected.

## Sources

| Source | Date | What it holds |
|--------|------|---------------|
| Live fetch of six `feeds.rssblue.com` feeds | 2026-09-23 | Four feeds that the corrective pass refused, and two accepted feeds as a control |
| `stophammer-crawler/analysis/data/feed_audit.ndjson` | Fetched 2026-03-26 to 2026-03-27 | The RSS text of 7,538 feeds that the index accepted |
| `src/verifiers/v4v_payment.rs`, `stophammer-parser/src/engine.rs` | Commit `a4f05a1` | The present rule and the parser mapping |

The measurements apply the test that `validate_routes` applies. A route is
valid when the address is not empty and the split is more than 0.

## Finding 1: The Refused Feeds Carry Complete Item-Level Blocks

| Feed | Channel recipients | Items | Items with a valid own block | Result |
|------|-------------------:|------:|-----------------------------:|--------|
| `silver-lining` | 0 | 1 | 1 | Refused |
| `rocket-paint` | 0 | 1 | 1 | Refused |
| `villian` | 0 | 1 | 1 | Refused |
| `cold-hearted` | 0 | 1 | 1 | Refused |
| `slight-curiosity` | 2 | 1 | 1 | Accepted |
| `oslosnowe` | 2 | 8 | 8 | Accepted |

Each of the four refused feeds declares `podcast:medium` as `music`. Each one
covers every track with a valid block of its own. None of them declares a
channel-level block.

The block in `silver-lining` is complete:

```xml
<podcast:value type="lightning" method="keysend" suggested="0.00000005000">
  <podcast:valueRecipient name="Anders Fransson" type="lnaddress" address="culturedlullaby623234@getalby.com" split="90" />
  <podcast:valueRecipient name="Epoch Radio Hosting" type="lnaddress" address="epochhosting@epochwallet.xyz" split="8" />
  <podcast:valueRecipient name="Epoch Radio Services" type="lnaddress" address="epoch-radio-features@epochwallet.xyz" split="1" />
  <podcast:valueRecipient name="Phantom Power Music" type="lnaddress" address="radio@epochwallet.xyz" split="1" />
</podcast:value>
```

The four splits add to 100. The block names a method and a suggested amount.
This feed pays its track. The verifier refuses the feed because the block is on
the item.

## Finding 2: The Host Does Not Decide The Shape

`feeds.rssblue.com` serves 264 of the 7,538 feeds in the snapshot, and the
index accepted each one. The two control feeds above are rssblue feeds with a
channel-level block. The publisher decides where the block goes. The host does
not.

## Finding 3: The Fallback Is Used, So The Feed-Level Requirement Must Stay

These counts come from the 7,491 accepted music feeds in the snapshot:

- 227 feeds (3.0 percent) hold at least one track that declares no value block.
- 568 of 23,894 tracks (2.4 percent) declare no block of their own.
- 0 feeds hold a track that declares a block with no valid recipient.

Those 227 feeds need the feed-level block, because 568 tracks resolve through
it. Decision point 3 of ADR 0048 keeps the requirement for them. The rejected
alternative that accepts any feed with a route anywhere would remove the only
route those tracks have.

## Finding 4: The Verifier Already Receives The Track Routes

`stophammer-parser/src/engine.rs:221` calls `extract_payment_routes(item)` and
stores the result in `track.payment_routes`. The parser already extracts an
item-level block. `V4VPaymentVerifier` already reads `track.payment_routes` in
its per-track loop. It returns before that loop when `feed_payment_routes` is
empty.

ADR 0048 needs a change in `src/verifiers/v4v_payment.rs` only. It needs no
parser change, no storage change and no protocol change.

## What This Record Does Not Measure

This record does not give the size of the affected population. The node does
not store a verifier refusal. `src/schema.sql` and `migrations/` hold no table
for it. Therefore the run output of the corrective pass is the only record of
those refusals.

Two scripts take the count from a saved run. The crawler writes each outcome
to standard error, so the run must capture it.

```bash
cargo run --release -- refresh --concurrency 5 2>&1 | tee run.log

cd stophammer-crawler/analysis
./bin/refresh_log_summary.sh /path/to/run.log .
./bin/value_coverage.py --urls ./v4v-rejected.txt --delay 2.0     --detail ./v4v-coverage.tsv
```

`refresh_log_summary.sh` groups the refusals by verifier and writes the refused
feed URLs. `value_coverage.py` fetches each URL and gives the count of feeds
that ADR 0048 accepts.

The snapshot cannot supply this count. It holds only feeds that the index
accepted, and therefore it shows no refusal. The same script reads the snapshot
with `--ndjson`, and it reproduces each number in this record.

## Conclusion

The evidence supports ADR 0048. Four of four refused feeds cover every track
with a valid item-level block. The verifier already receives the routes that
prove it. The condition that ADR 0048 set for its own rejection did not occur.

The evidence does not give the gain in feed count. That number needs the run
output of the corrective pass.
