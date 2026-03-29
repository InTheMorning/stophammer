# stophammer-crawler

Crawler release bundle.

Contents:

- crawler runtime: `stophammer-crawler`
- packaged deployment assets for gossip and optional one-shot crawl/import runs

Install shape:

- copy `bin/*` into your binary path
- copy `systemd/*` into `/usr/lib/systemd/system/`
- copy `env/*.example` to `/etc/stophammer/` and remove the `.example` suffix
- copy `sysusers.d/*` and `tmpfiles.d/*` into the matching system locations for
  packaged installs

The intended long-running unit is:

- `stophammer-gossip.service`

Optional example one-shot units are also included:

- `stophammer-import.service`
- `stophammer-import.timer`
- `stophammer-crawl.service`
- `stophammer-crawl.timer`
