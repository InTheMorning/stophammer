//! Incremental resolver subsystem.
//!
//! The current staged rollout keeps inline canonical rebuild behavior in place
//! and adds a durable queue plus worker for retryable canonical sync and
//! targeted artist identity cleanup.

pub mod queue;
pub mod worker;
