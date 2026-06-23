//! `runner-core` — the pure core of `flexnetos_runner` (ADR-0008 §2).
//!
//! - [`agent`] — agent-backend selector (Claude/Codex/Kimi; Claude is the default). *Which*
//!   coding agent `atc` drives for an agent-class job — orthogonal to the kernel choice.
//! - [`jobspec`] — the HMAC-signed job spec the App dispatches over UDS (S7).
//! - [`router`] — delegate-only kernel router (loop_lib/atc/handoff/weave); decides *what*
//!   runs *where*, never *how* (it never reimplements a kernel).
//! - [`safety`] — fork-PR isolation policy + runner rails (the §6 minimums).
//! - [`lifecycle`] — JIT/ephemeral runner lifecycle (one job, then removed).
//! - [`wire`] — the signed UDS dispatch frame (App → dispatcher) + reply.
//!
//! No process execution at this layer; `runner-actions` (Actions supervisor) and
//! `runner-dispatch` (UDS server) drive these typed seams.

pub mod agent;
pub mod error;
pub mod jobspec;
pub mod lifecycle;
pub mod router;
pub mod safety;
pub mod wire;

pub use agent::{Agent, ApiStyle};
pub use error::{CoreError, Result};
pub use wire::{sign_frame, verify_frame, DispatchRequest, DispatchResponse, WireError};
