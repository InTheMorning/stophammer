# stophammer-node

Community-node release bundle.

Contents:

- node runtime: `stophammer`
- packaged deployment assets for community-node operation only

Install shape:

- copy `bin/*` into your binary path
- copy `systemd/*` into `/usr/lib/systemd/system/`
- copy `env/*.example` to `/etc/stophammer/` and remove the `.example` suffix
- copy `sysusers.d/*` and `tmpfiles.d/*` into the matching system locations for
  packaged installs

The intended long-running unit is:

- `stophammer-community.service`
