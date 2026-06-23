//! `runner-core` — the pure core of `flexnetos_runner` (ADR-0008 §2).
//!
//! - [`agent`] — agent-backend selector (Claude/Codex/Kimi; Claude is the default). *Which*
//!   coding agent `atc` drives for an agent-class job — orthogonal to the kernel choice.
//! - [`jobspec`] — the HMAC-signed job spec the App dispatches over UDS (S7).
//! - [`router`] — delegate-only kernel router (loop_lib/atc/handoff/weave); decides *what*
//!   runs *where*, never *how* (it never reimplements a kernel).
//! - [`safety`] — fork-PR isolation policy + runner rails (the §6 minimums).
//! - [`loopguard`] — loop-detection circuit breaker (runaway-loop guard at the dispatch choke
//!   point; adapted from kclaw0 `loop-detection.js`).
//! - [`constitution`] — constitution-immutability gate (refuse to dispatch if the runner's own
//!   governing files change mid-run; adapted from `automaton` + kclaw0 `dark-factory.js`).
//! - [`cost`] — per-job cost report (the `atc → runner` cost seam; tokens + USD).
//! - [`governor`] — dispatch budget governor (bounded-autonomy kill-switch over jobs/tokens/USD;
//!   adapted from kclaw0 `dark-factory.js::enforceBudget` + `survival.js`).
//! - [`events`] — structured dispatch audit log (NDJSON event trail; adapted from kclaw0
//!   `event-system.js`).
//! - [`lifecycle`] — JIT/ephemeral runner lifecycle (one job, then removed).
//! - [`wire`] — the signed UDS dispatch frame (App → dispatcher) + reply.
//!
//! No process execution at this layer; `runner-actions` (Actions supervisor) and
//! `runner-dispatch` (UDS server) drive these typed seams.

pub mod agent;
pub mod constitution;
pub mod cost;
pub mod error;
pub mod events;
pub mod governor;
pub mod jobspec;
pub mod lifecycle;
pub mod loopguard;
pub mod router;
pub mod safety;
pub mod wire;

pub use agent::{Agent, ApiStyle};
pub use constitution::{Constitution, ConstitutionStatus};
pub use cost::JobCost;
pub use error::{CoreError, Result};
pub use events::{DispatchEvent, EventSink, NullSink, Outcome};
pub use governor::{Admission, Budget, Governor, Spend};
pub use loopguard::{fingerprint, LoopGuard, Verdict};
pub use wire::{sign_frame, verify_frame, DispatchRequest, DispatchResponse, WireError};
