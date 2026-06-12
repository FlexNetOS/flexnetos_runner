//! `runner-core` — the pure core of `flexnetos_runner` (ADR-0008 §2).
//!
//! - [`jobspec`] — the HMAC-signed job spec the App dispatches over UDS (S7).
//! - [`router`] — delegate-only kernel router (loop_lib/atc/handoff/weave); decides *what*
//!   runs *where*, never *how* (it never reimplements a kernel).
//! - [`safety`] — fork-PR isolation policy + runner rails (the §6 minimums).
//! - [`lifecycle`] — JIT/ephemeral runner lifecycle (one job, then removed).
//!
//! No process execution at this layer; `runner-actions` (Actions supervisor) and
//! `runner-dispatch` (UDS server) drive these typed seams.

pub mod error;
pub mod jobspec;
pub mod lifecycle;
pub mod router;
pub mod safety;

pub use error::{CoreError, Result};
