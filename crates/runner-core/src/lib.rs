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
//! - [`approval`] — human-approval admission policy (hold a flagged job class until a human grants
//!   approval; adapted from `Archon`'s `ApprovalNode` + `attractor`'s `wait.human`).
//! - [`lint`] — structural JobSpec lint (refuse a malformed job before it reaches a kernel;
//!   adapted from `attractor`'s VALIDATE phase).
//! - [`recovery`] — declarative recovery routing (retry-with-backoff vs. escalate-to-human advice
//!   for a failed dispatch; adapted from `attractor`'s `retry_target` / `wait.human`).
//! - [`cost`] — per-job cost report (the `atc → runner` cost seam; tokens + USD).
//! - [`governor`] — dispatch budget governor (bounded-autonomy kill-switch over jobs/tokens/USD;
//!   adapted from kclaw0 `dark-factory.js::enforceBudget` + `survival.js`).
//! - [`events`] — structured dispatch audit log (NDJSON event trail; adapted from kclaw0
//!   `event-system.js`).
//! - [`lifecycle`] — JIT/ephemeral runner lifecycle (one job, then removed).
//! - [`workspace`] — isolated-workspace teardown guarantee (cleanup runs on every exit path, fail
//!   included; adapted from `Archon`'s "fail → delete the worktree, zero residue").
//! - [`wire`] — the signed UDS dispatch frame (App → dispatcher) + reply.
//!
//! No process execution at this layer; `runner-actions` (Actions supervisor) and
//! `runner-dispatch` (UDS server) drive these typed seams.

pub mod agent;
pub mod approval;
pub mod constitution;
pub mod cost;
pub mod error;
pub mod events;
pub mod governor;
pub mod jobspec;
pub mod lifecycle;
pub mod lint;
pub mod loopguard;
pub mod recovery;
pub mod router;
pub mod safety;
pub mod wire;
pub mod workspace;

pub use agent::{Agent, ApiStyle};
pub use approval::ApprovalPolicy;
pub use constitution::{Constitution, ConstitutionStatus};
pub use cost::JobCost;
pub use error::{CoreError, Result};
pub use events::{DispatchEvent, EventCategory, EventSink, NullSink, Outcome};
pub use governor::{Admission, Budget, Governor, Spend, SurvivalTier};
pub use lint::{is_structurally_valid, structural_errors, LintError};
pub use loopguard::{fingerprint, LoopGuard, Verdict};
pub use recovery::{FailureKind, RecoveryDirective, RecoveryPolicy, RecoveryVerb, RetryLedger};
pub use wire::{sign_frame, verify_frame, Approval, DispatchRequest, DispatchResponse, WireError};
pub use workspace::{JobWorkspace, NoopWorkspaceProvider, TeardownReport, WorkspaceProvider};
