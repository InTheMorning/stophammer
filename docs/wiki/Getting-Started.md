# Getting Started

## What to Read First

If you are new to the repo:

1. read [user-guide.md](/home/citizen/build/stophammer/docs/user-guide.md)
2. skim [README.md](/home/citizen/build/stophammer/README.md)
3. then choose either [operations.md](/home/citizen/build/stophammer/docs/operations.md)
   or [API.md](/home/citizen/build/stophammer/docs/API.md)

## Fastest Local Start

Build the binary:

```bash
cargo build --release
```

Run a local primary:

```bash
DB_PATH=./stophammer.db \
KEY_PATH=./signing.key \
CRAWL_TOKEN=change-me \
SYNC_TOKEN=change-me \
BIND=127.0.0.1:8008 \
./target/release/stophammer
```

Check that it is up:

```bash
curl http://127.0.0.1:8008/health
curl http://127.0.0.1:8008/node/info
```

## Most Important Concepts

- the primary ingests, verifies, signs, and serves
- community nodes replicate and serve read-only
- crawlers run as a separate package directory and release artifact
- the current public API is source-first, not canonical-first

## Before You Push Code

Use the same gate the repo now treats as mandatory:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --tests
```
