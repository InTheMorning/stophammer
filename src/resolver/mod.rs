//! Incremental resolver subsystem.
//!
//! Phase 1 keeps the current inline canonical rebuild behavior in place and
//! adds a durable queue plus worker for retryable canonical sync.

pub mod queue;
pub mod worker;
