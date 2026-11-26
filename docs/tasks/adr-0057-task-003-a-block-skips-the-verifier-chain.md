# ADR 0057 Task 003: A Blocking Body Skips The Verifier Chain

Owner: [ADR 0057](../adr/archive/0057-a-feed-can-block-this-index.md) §3,
which its tests own now, and
[ADR 0101](../adr/0101-a-publisher-feed-can-list-playlists.md) §4.

Repositories: `stophammer` and `stophammer-crawler`. Each repository gets its
own commit.

## Operator Choice

Operator choice 2026-11-26, sent through the studio session: the two
corrections below make the code obey ADR 0057 §3 and ADR 0101 §4. They do not
necessitate a new ADR. The operator does not retire the test feed with
`DELETE /v1/feeds/{guid}`, so the Studio retire flow gets a full test after
the deploy.

## Defect

The Studio human check of 2026-11-26 found that the label feed
`84e85b32-f0dc-5d1e-952e-13a438685954` stays in the index after two
podpings. Its new body has the medium `publisher`, `podcast:block` `yes` and
no `remoteItem`.

1. The node runs the verifier chain before the block rule. `medium_music`
   refuses a publisher feed with no music `remoteItem`, so the Update case
   that retires the record does not run. Only the GUID change path skipped
   the chain for a blocking body (ADR 0106 §8 step 5).
2. The gossip crawler then skips each subsequent podping of the URL. Its feed
   memory skips a prior medium gate refusal, also for a publisher feed.
   The shared skip list does not skip a publisher feed.

## Goal

- The node: when the case is Update and the body blocks this index, the
  verifier chain does not run. The write phase retires the record.
- The crawler: `prior_medium_gate_skip` in `src/feed_skip.rs` holds the rule
  for a prior medium gate refusal. The shared skip list and the gossip feed
  memory both use it, so a refused publisher feed gets the next podping.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Read phase chain skip | extend | `src/api.rs` | The GUID change path skips the chain for a blocking body. The Update case uses the same flag |
| Block rule | reuse | `src/ingest.rs` | `source_blocks_this_index` decides if a body blocks this index |
| Medium gate skip rule | extend | `stophammer-crawler/src/feed_skip.rs` | The shared skip list held the rule. It moves into `prior_medium_gate_skip`, and the gossip memory calls it |
| ADR 0057 test helpers | extend | `tests/adr0057_podcast_block_tests.rs` | `test_app_state` gets a form that takes the chain names |

## Acceptance Criteria

Each criterion is mechanical.

1. A chain with `medium_music` admits a publisher feed. The feed then sends
   a body with an unbounded `yes` block and no `remoteItem`. The answer is
   `source_blocked`, and the node has no `feeds` row for its GUID. Test:
   `update_block_retires_a_body_that_a_verifier_refuses` in
   `tests/adr0057_podcast_block_tests.rs`. It fails on the code before this
   task with the `[medium_music]` refusal.
2. The gossip feed memory does not skip a publisher URL with a prior
   `[medium_music]` refusal. Test:
   `should_not_skip_publisher_medium_gate_rejection` in
   `stophammer-crawler/src/modes/gossip.rs`. It fails on the code before this
   task.
3. The other medium gate skips do not change. Tests:
   `should_skip_medium_gate_rejection` in the gossip module, and the tests of
   `src/feed_skip.rs`.

## Open Checks

- After the deploy, the operator sends a podping of the label feed from
  Studio. `GET /v1/feeds/84e85b32-f0dc-5d1e-952e-13a438685954` then gives
  `404`.
