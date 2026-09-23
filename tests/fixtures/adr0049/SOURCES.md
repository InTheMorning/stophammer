# ADR 0049 Task 002: Fixture Sources

This file records each fetch for the real-feed fixtures in this directory.
[ADR 0049](../../../docs/adr/0049-publisher-relationships-are-rss-facts.md)
§9 names these feeds. The parser commit for each `.feed_data.json` file is
`c18b70d14fe04d610981009f454477382f7dcb62`.

## Fetched Files

| Fixture | URL | Fetch time (UTC) | HTTP status | SHA-256 of `.xml` |
|---|---|---|---|---|
| `detox-artist` | `https://wavlake.com/feed/artist/137aaa9c-75ff-4916-9f23-e02968b2d15e` | 2026-09-23T17:32:45Z | 200 | `9c8f68ac08cad2c3607baf6c164c15265b2443eab33f042e058ac645599bb963` |
| `detox-album` | `https://wavlake.com/feed/music/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a` | 2026-09-23T17:32:56Z | 200 | `a59d1065a721c4374c4b38274740e7529db10b0927f42032a78f6a3c05aac124` |
| `rssblue-publisher` | `https://publishers.rssblue.com/acoldtripnowhere` | 2026-09-23T17:33:19Z | 200 | `a3ba3fc5be93d3dff7bb8d348f95e1a9a12959b83ea8cbb523d7f4956c2f1c52` |
| `rssblue-album` | `https://feeds.rssblue.com/e-pluribus-unum` | 2026-09-23T17:33:30Z | 200 | `7a9db855b9b010eae47b6ea972d4565dad29a7b7b72bbfd2c959a35daaaf6e75` |
| `sirlibre-label` | `https://sirlibre.com/publisher/sir-libre-records-publisher-rss.xml` | 2026-09-23T17:33:36Z | 200 | `681cd4f7946f962ce337cafb48abcd6086d3ebd7151b816ede293124dec3dde0` |
| `sirlibre-album` | `https://cdn.kolomona.com/podcasts/lightning-thrashes/faces-pale/reap-what-you-sow/reap-what-you-sow.xml` | 2026-09-23T17:33:45Z | 200 | `5f4d62f12920e5724b80f46ad585c10e1e9dbccf6f49a5647f975d6c1ae34e87` |
| `jimmyv-publisher` | `https://music.jimmyv4v.com/publisher/jimmy-v-publisher-rss.xml` | 2026-09-23T17:33:51Z | 200 | `1796965c608e823d34d482d3fc3820c0cc47d776ebdea78f45a8f04afcfe67a3` |
| `jimmyv-produced-album` | `https://www.wavlake.com/feed/a047ea02-7c6b-4180-939d-6207ab17b9aa` | 2026-09-23T17:34:05Z | 200 | `b109f165a8b0cfc5c29e6e477ad485e22459c43f3940c58fc553d851e24e7de7` |
| `no-publisher-album` | `https://feed.justcast.com/shows/into-the-blue/audioposts.rss` | 2026-09-23T17:34:16Z | 200 | `0f3861fba106d84ff5c51d0fc7edca1d9c7992a5763a4280561e9fc9b94b3770` |

## Derived Target URLs

The task names two rows by rule, not by a fixed URL. This record gives the
URL that the rule picked.

- `rssblue-album` is the first `feedUrl` with `medium="music"` in
  `rssblue-publisher.xml`. The feed carries two such items. The Wavlake item
  was not fetched, because it was not first.
- `sirlibre-album` is the first `feedUrl` with `rel="label"` in
  `sirlibre-label.xml`. Each item in that feed carries `rel="label"`, so
  this is the first item overall.
- `jimmyv-produced-album` is the `feedUrl` with `rel="producer"` in
  `jimmyv-publisher.xml`. The feed lists eleven such items. This record
  fetched the first one.

## The Duplicate Wavlake URL

The task named a second URL for the `detox-album` `podcast:guid`:
`https://wavlake.com/feed/cf3fb24c-582c-45dd-8ac7-bb41cdf4d41a`. This record
fetched it one time, at 2026-09-23T17:33:08Z, and got HTTP status 200. This
record did not keep the body.

The SHA-256 of that body was
`5461971d442a168f3fc8dc48c9736095b78ee968e77b5b0af31d1a995ff7e8ce`. This
differs from the SHA-256 of the stored `detox-album.xml`. A line-by-line
compare found one difference: the `lastBuildDate` value. Wavlake sets this
value to the current time on each fetch.

Each other line was equal. The `podcast:guid` line was also equal. So the
two URL forms carry the same feed content, but not the same byte-for-byte
body.

## A Deviation: The Fallback GUID For `no-publisher-album`

`no-publisher-album.xml` carries no `<podcast:guid>` tag. The
`stophammer-parse` binary refuses such a feed with no fallback GUID:

```text
{"error": "missing podcast:guid and no fallback configured"}
```

The plain command from the task packet gives an empty `.feed_data.json`
file for this one fixture. This record used the CLI's `--fallback-guid`
flag. This record calculated the flag's value. The parser did not
calculate it:

```bash
cargo run --quiet --manifest-path stophammer-parser/Cargo.toml \
  --bin stophammer-parse -- \
  --fallback-guid 48ad9b64-d3a8-5719-b452-9ae85e57ab54 \
  < tests/fixtures/adr0049/no-publisher-album.xml | jq . \
  > tests/fixtures/adr0049/no-publisher-album.feed_data.json
```

The value `48ad9b64-d3a8-5719-b452-9ae85e57ab54` is a UUIDv5. Its namespace
is `ead4c236-bf58-58c6-a2c6-a6b28d128cb6`, the Podcast Namespace fallback-GUID
namespace. Its name is `feed.justcast.com/shows/into-the-blue/audioposts.rss`,
the fetch URL with its scheme removed. This is the same derivation the
Podcast Namespace specification recommends for a feed with no stated GUID.

Each other `.feed_data.json` file in this directory came from the plain
command in the task packet only.

This deviation matters because of the crawler's own fallback-GUID rule.
`stophammer-crawler`'s `feed`, `gossip` and `refresh` modes pass no fallback
GUID today. Only `ndjson` mode passes one, and only from a GUID stored in
the database from an earlier ingest. So a first crawl of this URL, through
the crawler as it stands, finds the same parse error this record found.
This record does not change the crawler. It only reports the result.
