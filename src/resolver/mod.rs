//! Incremental resolver subsystem.
//!
//! The current staged rollout uses a durable queue plus worker for retryable
//! source-read-model, canonical, and targeted artist-identity cleanup.
//!
//! The next replication stage is primary-authority resolved-state emission:
//! `resolverd` can begin producing signed canonical snapshot events without
//! changing the preserved source-layer contract.

pub mod queue;
pub mod worker;
