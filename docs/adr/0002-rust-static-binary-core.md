# ADR 0002: Use Rust for the Core Node Binary

## Status
Accepted

## Context
The stophammer node needs to run on commodity VPS instances (1–2 vCPU, 512 MB RAM) contributed by community members in a BOINC-style drop-in deployment. The primary constraints are:

- Minimal memory footprint at idle so multiple services can coexist
- Zero system dependency installation — operators just download and run
- High single-threaded throughput for SQLite writes (single-writer model)
- Easy cross-compilation to Linux (x86_64 and aarch64) from macOS CI

Alternatives considered:
- **Bun/Node.js**: ~180 MB idle RSS, requires Node runtime on host
- **Go**: Good cross-compilation, ~20 MB idle, but GC pauses and less predictable latency
- **Python**: Not suitable for a long-running network service at this performance target

## Decision
We will implement the core node in Rust, compiled as a static binary using `rusqlite` with the `bundled` feature (SQLite compiled in) and targeting `*-unknown-linux-musl` for zero system dependencies. The idle footprint is approximately 8 MB RSS.

## Consequences
- The release binary is fully self-contained — no SQLite, no libc, no runtime required on the host.
- Cross-compilation to musl targets requires a one-time CI toolchain setup.
- Crawler clients (which do RSS fetching, XML parsing, and HTTP calls) can be written in any language; they communicate with the core via HTTP.
- Rust's ownership model eliminates an entire class of concurrency bugs at the cost of a steeper initial development curve.
