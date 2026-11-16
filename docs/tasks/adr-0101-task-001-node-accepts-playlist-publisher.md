# ADR 0101 Task 001: The Node Accepts A Publisher Feed That Lists Playlists

Owner: [ADR 0101](../adr/0101-a-publisher-feed-can-list-playlists.md) §1.

Repository: `stophammer`. This task does not need task 002. The two tasks can
run at the same time, because they change different repositories.

## Goal

`MediumMusicVerifier` accepts a publisher feed that has a minimum of one
channel `podcast:remoteItem` with the medium `music` or `musicL`. A publisher
feed with neither medium stays rejected.

## Base components

| Component | Status | Location | Reason |
|---|---|---|---|
| Medium helpers | reuse | `src/medium.rs` | `is_music` and `is_musicl` compare with the ASCII case ignored |
| Medium verifier | extend | `src/verifiers/medium_music.rs` | The publisher branch also accepts a `musicL` item |
| Verifier tests | extend | `tests/coverage_gap_tests.rs` | Sections 16 and 26 already build an `IngestContext` for this verifier |
| Ingest test helpers | reuse | `tests/musicl_api_tests.rs` | `test_app_state_with_chain`, `json_request` and `body_json` send an ingest to the router |

## Files To Inspect

- `src/verifiers/medium_music.rs`: `MediumMusicVerifier::verify` and the module
  comment
- `src/medium.rs`
- `src/verify.rs`: the verifier table in the module comment
- `tests/coverage_gap_tests.rs`: sections 16 and 26
- `tests/musicl_api_tests.rs`: the helpers and
  `ingest_musicl_discards_tracks_and_skips_resolver_queue`
- `tests/adr0038_tests.rs`: an ingest body of a publisher feed with
  `remote_items`
- `docs/verifier-guide.md`: the text on `medium_music`

## Files Likely To Change

- `src/verifiers/medium_music.rs`
- `src/verify.rs`: only the verifier table, if it states the publisher rule
- `tests/coverage_gap_tests.rs`
- New: `tests/adr0101_playlist_publisher_tests.rs`
- `docs/verifier-guide.md`

## Do Not Touch

- `src/query.rs` and the `publisher` view. ADR 0101 §5 keeps the read side.
- `src/db.rs`, the migrations, the events and the payment verifier.
- `stophammer-parser/` and `stophammer-crawler/`.
- The ADR files, the ADR index and `AGENTS.md`.

## Constraints

- Use `medium::is_music` and `medium::is_musicl` for the item medium. Do not
  compare strings inline.
- The new rejection reason is
  `publisher feed has no remoteItem with medium='music' or 'musicL'`.
  The verifier adds the `[medium_music]` prefix as before.
- Update the comment of the publisher branch and the module comment. Each
  comment names ADR 0101 §1.
- Read the `rust-dev` skill rules that `AGENTS.md` names.
- Write each comment and each document line in Simplified Technical English.
  Read `/home/citizen/.agents/skills/asd-ste100/SKILL.md` first.
- Do not commit. Do not run a Git command that writes.

## Acceptance Criteria (mechanical)

1. A test of `MediumMusicVerifier` in `tests/coverage_gap_tests.rs`: a
   publisher feed with only `medium="musicL"` items gives `VerifyResult::Pass`.
2. The same test file: a publisher feed with only `medium="MusicL"` items
   gives `VerifyResult::Pass`.
3. The same test file: a publisher feed with only `medium="podcast"` items
   gives `VerifyResult::Fail`, and the reason names `music` and `musicL`.
4. The existing tests of sections 16 and 26 pass with no change of their
   assertions, except for the text of the rejection reason.
5. A router test in `tests/adr0101_playlist_publisher_tests.rs`: an ingest of
   a publisher feed with two `musicL` remote items and no `music` item gives
   `200`. `GET /v1/feeds/{guid}?include=remote_items` then gives `raw_medium`
   `publisher` and the two entries with `medium` `musicL`. Model the body on
   the ChadF publisher feed of ADR 0101 §Context.

## Checks

```bash
cargo build && cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Report

Give the changed files and the result of each check. Name each point where the
code did not let you follow this packet.
