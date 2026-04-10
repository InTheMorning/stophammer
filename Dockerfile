# Runtime image for stophammer indexer/community roles.
# Build all main-workspace binaries, then copy the runtime subset into a minimal image.

FROM rust:alpine AS chef

RUN apk add --no-cache \
    build-base \
    cmake \
    linux-headers \
    musl-dev \
    perl \
    pkgconf \
 && cargo install cargo-chef

WORKDIR /build

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --bins --recipe-path recipe.json

COPY . .
RUN cargo build --release --bin stophammer --bin rebuild_search

# ── Runtime ────────────────────────────────────────────────────────────────────

FROM alpine:3.20 AS stophammer-runtime

RUN apk add --no-cache ca-certificates \
 && addgroup -S stophammer \
 && adduser -S -G stophammer stophammer \
 && mkdir -p /data \
 && chown stophammer:stophammer /data

WORKDIR /data
COPY --from=builder /build/target/release/stophammer /usr/local/bin/stophammer
COPY --from=builder /build/target/release/rebuild_search /usr/local/bin/rebuild_search

USER stophammer

ENV DB_PATH=/data/stophammer.db
ENV KEY_PATH=/data/signing.key
ENV BIND=0.0.0.0:8008

EXPOSE 8008

FROM stophammer-runtime AS stophammer-indexer
CMD ["stophammer"]

FROM stophammer-runtime AS stophammer-node

ENV NODE_MODE=community

CMD ["stophammer"]
