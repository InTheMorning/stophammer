//! Stophammer: a decentralised music feed index with value-for-value (V4V) verification.
//!
//! Stophammer ingests podcast-namespace RSS feeds, validates them through a
//! configurable [`verify::VerifierChain`], persists canonical feed/track/artist
//! data, and replicates it to community nodes via a signed event log.
//!
//! # Module layout
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`api`] | Axum HTTP router, handlers, and shared application state |
//! | [`apply`] | Idempotent application of signed events to the local database |
//! | [`community`] | Community (replica) node sync, push-receive, and tracker registration |
//! | [`db`] | `SQLite` schema, queries, and connection helpers |
//! | [`event`] | Signed event envelope and serialisation |
//! | [`ingest`] | Crawler submission types (`IngestFeedRequest` / `IngestResponse`) |
//! | [`model`] | Core domain types: `Artist`, `Feed`, `Track`, `PaymentRoute`, etc. |
//! | [`proof`] | Proof-of-possession challenge/token flow (RFC 8555-inspired) |
//! | [`quality`] | Feed quality scoring heuristics |
//! | [`query`] | Read-only query routes (`/v1/feeds`, `/v1/tracks`, etc.) |
//! | [`search`] | Full-text search via `SQLite` FTS5 |
//! | [`signing`] | Ed25519 node identity key management |
//! | [`sync`] | Event log pagination and push/pull sync protocol |
//! | [`tls`] | Automatic TLS via ACME (Let's Encrypt) |
//! | [`verify`] | Feed-ingest verification pipeline and `VerifierChain` |
//! | [`verifiers`] | Built-in verifier implementations (plugin pattern) |
//!
//! # Key entry points
//!
//! - **Primary mode**: `main::run_primary` builds the full router from [`api::build_router`].
//! - **Community mode**: `main::run_community` builds a read-only router merged
//!   with [`community::build_community_push_router`].
//! - **Verification**: [`verify::build_chain`] assembles the chain from env config.

pub mod api;
pub mod apply;
pub mod community;
pub mod db;
pub mod db_pool;
pub mod event;
pub mod ingest;
pub mod model;
pub mod proof;
pub mod quality;
pub mod query;
pub mod resolver;
pub mod search;
pub mod signing;
pub mod sync;
pub mod tls;
pub mod verifiers;
pub mod verify;
