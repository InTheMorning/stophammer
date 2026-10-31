# ADR 0081 Task 001: The PodcastIndex API Client

Owner: [ADR 0081](../adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md)
§1, §2 and §3. The phase plan is
[the music list plan](../plans/podcastindex-music-list-phase-plan.md).

Repository: `stophammer-crawler`. The operator commits. This task has no
dependency on another task.

## Goal

A module `src/podcastindex_api.rs` reads the credentials from the
environment and signs one request to `GET /podcasts/bymedium?medium=music`.
It gives the rows of the list as `id`, `url` and `podcastGuid`. No value of the
credentials reaches a log, an error text or a `Debug` output.

## Files To Inspect

- `src/crawl.rs`: `CrawlConfig::from_env_with_force` (line 61) and
  `user_agent` (line 33), for the user agent value
- `src/modes/refresh.rs`: `fetch_feeds_recent_page` (line 85), for the form of
  a small HTTP read with `reqwest`
- `src/crawl.rs` tests, near line 1877: the `TcpListener` stub pattern
- `Cargo.toml`: the dependencies
- `AGENTS.md` of the crawler: the lint set and the test convention

## Files Likely To Change

- `src/podcastindex_api.rs`, new
- `src/main.rs`: one `mod podcastindex_api;` line
- `Cargo.toml` and `Cargo.lock`: `sha1 = "0.10"`

## Constraints

- **The credentials.** A struct `Credentials { key: String, secret: String }`
  with `Credentials::from_env() -> Result<Self, String>`. It reads
  `PODCASTINDEX_API_KEY` and `PODCASTINDEX_API_SECRET`. A missing or empty
  value gives the error text
  `PODCASTINDEX_API_KEY and PODCASTINDEX_API_SECRET must be set`, with no
  value in it. Write `impl fmt::Debug for Credentials` by hand, and print
  `Credentials { key: "<redacted>", secret: "<redacted>" }`. Derive no
  `Clone`, `Serialize` or `Display`.
- **The header value.** `pub fn authorization_hex(key: &str, secret: &str,
  date: &str) -> String` gives the SHA-1 of the bytes of `key`, `secret` and
  `date` in that sequence, as 40 lowercase hex digits. Use the `sha1` crate
  of RustCrypto and the `hex` crate that the crate already has.
- **The headers.** `pub fn auth_headers(credentials: &Credentials, unix_time:
  i64) -> [(&'static str, String); 3]` gives `X-Auth-Key`, `X-Auth-Date` as
  the integer text, and `Authorization`. The caller passes the time, so a
  test can fix it.
- **The request.** `pub async fn fetch_music_list(client: &reqwest::Client,
  api_base: &str, credentials: &Credentials, user_agent: &str) ->
  Result<Vec<MusicListRow>, String>`.
  - The URL is `{api_base}/podcasts/bymedium?medium=music`.
  - The request has the three headers and `User-Agent: {user_agent}`.
  - The function reads the body as bytes and refuses a body over 64 MiB with
    an error.
  - A status other than 200 gives an error with the status and the first 200
    bytes of the body.
  - The error text never holds a header value.
- **The default base.** `pub const DEFAULT_API_BASE: &str =
  "https://api.podcastindex.org/api/1.0"`.
- **The parse.** `pub fn parse_music_list(body: &[u8]) ->
  Result<Vec<MusicListRow>, String>`.
  - The body is a JSON object with `status`, `feeds` and `count`.
  - A `status` other than the text `"true"` or the boolean `true` gives an
    error. The error holds the `description` field when it is present.
  - Each row of `feeds` gives
    `MusicListRow { id: i64, url: String, podcast_guid: Option<String> }`.
  - A row with no `id` or no `url` is an error, because the list is then not
    the list that the probe saw.
  - A missing or null `podcastGuid` gives `None`. Other fields are ignored.
- **The client.** The function takes a `reqwest::Client` from the caller.
  Task 004 builds it with the default redirect policy, because the API host
  is not a feed host. The `PublicOnlyResolver` is not used for the API.
- **Comments.** The module doc names ADR 0081 §1 to §3. Each public item has a
  doc comment.
- **Lints.** `[lints.clippy] pedantic = "deny"`. Each new `#[expect]` has a
  reason.

## Do Not Touch

- `src/modes/`, `src/crawl.rs`, `src/feed_skip.rs`, `src/follow.rs`
- The feed fetch client and `PublicOnlyResolver`
- `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- Git: run no git write command. The operator commits.

## Implementation Steps

1. Add `sha1 = "0.10"` to `Cargo.toml`. Run `cargo build`, and confirm that
   `Cargo.lock` gets the `sha1` crate and no second `digest` version.
2. Write `Credentials`, its `from_env` and its `Debug`.
3. Write `authorization_hex` and `auth_headers`, with their tests.
4. Write `MusicListRow` and `parse_music_list`, with their tests.
5. Write `fetch_music_list`, with one stub-server test.
6. Add the module line in `src/main.rs`.

## Acceptance Criteria

Mechanical. Each is an inline test in `src/podcastindex_api.rs`.

- ADR 0081 check 1: `authorization_hex("UXKCGDSYGUUEVQJSYDZH",
  "yzJe2eE7XV-3eY576dyRZ6wXyAbndh6LUrCZ8KN|", "1613713388")` gives
  `73a1fffed61c1d30d858beb1fc48f355386449d2`.
- `auth_headers` gives the three names in the sequence `X-Auth-Key`,
  `X-Auth-Date`, `Authorization`, with the date as the integer text and the
  key as given.
- ADR 0081 check 2: the `Debug` text of a `Credentials` with the key
  `KEY-SECRET-TEST-1` and the secret `SECRET-VALUE-TEST-1` holds neither
  value. The error of `from_env` with no variables holds neither value.
- `parse_music_list` of a body with two rows, one with `podcastGuid` and one
  with `"podcastGuid": null`, gives two rows with `Some` and `None`.
- `parse_music_list` of a body with `"status": "false"` and a `description`
  gives an error that holds the description.
- `parse_music_list` of a body whose row has no `url` gives an error.
- `parse_music_list` of a body that is not JSON gives an error.
- `fetch_music_list` against a `TcpListener` stub that answers 200 and a
  two-row body gives two rows. The stub records the request, and the request
  has `X-Auth-Key`, `X-Auth-Date`, `Authorization` and `User-Agent`.
- `fetch_music_list` against a stub that answers 401 gives an error that
  holds `401` and not the key value.

## Test Commands

```bash
cd ~/build/stophammer/stophammer-crawler
cargo build
cargo test podcastindex_api
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```

## Escalation Triggers

Stop and report to the orchestrator, with no workaround, when:

- `cargo build` gives a second version of `digest` or `sha1` in `Cargo.lock`.
- The stub test cannot run because the crawler tests have no pattern for a
  local HTTP server that serves one body.
- A step needs a change to a file of "Do Not Touch".

## Expected Final Report

1. files changed
2. tests run and their results
3. behavior changed
4. deviations from this task
5. unresolved concerns

## Prompt for lower-context coding model

You are implementing one bounded task from a larger plan.

Implement only this task. Do not redesign the architecture.

Read:
- `~/build/stophammer/stophammer-crawler/AGENTS.md` (the lint set, the test convention and the gate)
- `~/build/stophammer/docs/adr/0081-a-daily-pass-reads-the-podcastindex-music-list.md`, §1, §2 and §3
- `~/build/stophammer/docs/tasks/adr-0081-task-001-api-client.md` (this task)
- the files of "Files To Inspect" in this task

Goal:
- A module `src/podcastindex_api.rs` reads the credentials from the environment, signs one request to the music list, and gives the rows as `id`, `url` and `podcastGuid`. No credential value reaches a log, an error text or a `Debug` output.

Constraints:
- Follow the sections "Constraints" and "Implementation Steps" of this task.
- Use the `sha1` crate of RustCrypto and the `hex` crate. Add no other dependency.
- Stop at each escalation trigger of this task.

Do not touch:
- `src/modes/`, `src/crawl.rs`, `src/feed_skip.rs`, `src/follow.rs`, the feed fetch client, `README.md`, `AGENTS.md`
- `stophammer/`, `stophammer-parser/`
- git: run no git write command

Acceptance criteria:
- Each item of the section "Acceptance Criteria" of this task is a passing test.

Test commands:
- `cargo build`
- `cargo test podcastindex_api`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

At the end, report:
1. files changed
2. tests run
3. behavior changed
4. deviations from task
5. unresolved concerns
