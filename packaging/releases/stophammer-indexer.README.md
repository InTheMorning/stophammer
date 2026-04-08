# stophammer-indexer

Primary/indexer release bundle.

Contents:

- node runtime: `stophammer`
- packaged deployment assets for primary/index operation

Install shape:

- copy `bin/*` into your binary path
- copy `systemd/*` into `/usr/lib/systemd/system/`
- copy `env/*.example` to `/etc/stophammer/` and remove the `.example` suffix
- copy `sysusers.d/*` and `tmpfiles.d/*` into the matching system locations for
  packaged installs

The intended long-running units are:

- `stophammer-primary.service`
