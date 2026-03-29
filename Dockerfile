# Runtime image for stophammer (primary or community node)
# Build the binary first via the builder stage, then copy into a minimal image.

FROM rust:1.87-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build
COPY . .

RUN cargo build --release --bins

# ── Runtime ────────────────────────────────────────────────────────────────────

FROM alpine:3.20

RUN addgroup -S stophammer && adduser -S -G stophammer stophammer

WORKDIR /app
COPY --from=builder /build/target/release/stophammer /app/stophammer
COPY --from=builder /build/target/release/stophammer-resolverd /app/stophammer-resolverd
COPY --from=builder /build/target/release/stophammer-resolverctl /app/stophammer-resolverctl
COPY --from=builder /build/target/release/backfill_canonical /app/backfill_canonical
COPY --from=builder /build/target/release/backfill_artist_identity /app/backfill_artist_identity
COPY --from=builder /build/target/release/backfill_wallets /app/backfill_wallets
COPY --from=builder /build/target/release/review_artist_identity /app/review_artist_identity
COPY --from=builder /build/target/release/review_artist_identity_tui /app/review_artist_identity_tui
COPY --from=builder /build/target/release/review_wallet_identity /app/review_wallet_identity
COPY --from=builder /build/target/release/review_wallet_identity_tui /app/review_wallet_identity_tui
COPY --from=builder /build/target/release/review_source_claims_tui /app/review_source_claims_tui

RUN mkdir -p /data && chown stophammer:stophammer /data

USER stophammer

ENV DB_PATH=/data/stophammer.db
ENV KEY_PATH=/data/signing.key
ENV BIND=0.0.0.0:8008

EXPOSE 8008

ENTRYPOINT ["/app/stophammer"]
