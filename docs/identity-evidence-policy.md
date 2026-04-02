# Identity Evidence Policy

This document records the current editorial policy for resolver review signals.

## Priority

Prefer explicit, rich feed information and known deterministic platform facts over sparse legacy metadata.

Current strength order:

1. shared canonical external IDs
2. shared `nostr_npub`
3. shared normalized website
4. same-feed structural evidence such as track/feed or contributor disagreement
5. wallet alias similarity
6. publisher-family context

## Artist Identity

- Same website is strong supporting evidence, but never a guarantee by itself.
- Shared `nostr_npub` is stronger than wallet-alias similarity.
- Publisher-family evidence is contextual only:
  - some artists publish themselves
  - some publisher feeds act like artist feeds
  - some represent families, collectives, or labels
  - some publishers are not artists at all
- Compound credits such as `A and B` or `A feat. B` should remain separate entities unless source data is repaired. They may raise review, but should not be collapsed into solo artists automatically.

## Wallet Ownership

- Same alias is useful but not decisive. Wallet aliases can be arbitrary, humorous, or platform-shaped.
- Stronger artist-owner hints come from explicit feed context:
  - dominant non-fee route on the feed
  - dominance repeated across feed tracks
  - alias or route naming that nearly matches the feed artist identity
- Wavlake routes are a special case:
  - route ownership is not meaningful for wallet-owner inference
  - platform-generated aliases such as `artist_name via wavlake` are deterministic feed metadata, not proof of self-custodied ownership

## Conflicts

- Conflicting canonical external IDs block `likely_same_artist`.
- Conflicting linked artists block `likely_wallet_owner_match`.
- Publisher-family context never overrides stronger explicit identity conflicts.
