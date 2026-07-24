//! `runner-core` — the pure core of `flexnetos_runner` (ADR-0008 §2).
//!
//! - [`agent`] — agent-backend selector (Claude/Codex/Kimi; Claude is the default). *Which*
//!   coding agent `atc` drives for an agent-class job — orthogonal to the kernel choice.
//! - [`jobspec`] — the HMAC-signed job spec the App dispatches over UDS (S7).
//! - [`router`] — delegate-only kernel router (loop_lib/atc/handoff/weave); decides *what*
//!   runs *where*, never *how* (it never reimplements a kernel), with deterministic route selection.
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
//! - [`quarantine`] — cross-dispatch quarantine of a repeatedly-failing job (terminal refuse after N
//!   kernel failures of the same fingerprint; adapted from `automaton`'s child `→ dead` lifecycle —
//!   the enforcement teeth behind recovery's escalate advice).
//! - [`deadline`] — per-job wall-clock deadline policy (bound a *hung* delegation by time — the axis
//!   the breaker/governor/quarantine don't cover; adapted from `attractor`'s `timeout` node +
//!   `Archon`/`kclaw0` per-op timeouts).
//! - [`redact`] — secret redaction for the audit-log + error-reply egress surfaces (scrub key
//!   material out of every operator-readable string before it is logged/returned; adapted from
//!   `Archon`'s `repo.ts` token scrub).
//! - [`ratelimit`] — windowed dispatch rate cap + per-route failure cooldown (bound the *rate* of
//!   distinct in-budget dispatches — the timing axis the breaker/governor/quarantine don't cover;
//!   adapted from `automaton`'s hourly/daily caps + 5-min error backoff; clock-injected).
//! - [`scan`] — pre-dispatch content/injection scan of the spec's free-text fields (severity-graded
//!   pattern bank + scan/decide split, fail-closed on a threshold; adapted from `Archon`'s
//!   `marketplace-security-scan.ts` + `kclaw0`'s `path-simulator.js` risk scoring).
//! - [`risk`] — history-calibrated pre-route risk score (advice-only: blend a static base rate with
//!   the live per-fingerprint failure rate, surfaced in the audit trail; adapted from `kclaw0`'s
//!   `path-simulator.js`). The soft, continuous companion to the hard breaker/quarantine latches.
//! - [`targets`] — delegation-target allowlist (kernel reachability registry for loop/atc/hf/weave;
//!   adapted from fail-closed allowlist / egress-control prior art).
//! - [`singleflight`] — per-target older-wins mutex for mutable repo work (the buildable seam of the
//!   max-in-flight/single-flight backlog item; global cap waits for concurrent serve).
//! - [`stategate`] — route-class × survival-tier admission matrix (defer non-essential work when
//!   the runner is conserving/distressed).
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
//! No process execution at this layer. The Nix-owned GitHub runner substrate and
//! `runner-dispatch` (UDS server) drive these typed seams.

pub mod agent;
pub mod approval;
pub mod authority;
pub mod constitution;
pub mod cost;
pub mod deadline;
pub mod error;
pub mod events;
pub mod governor;
pub mod jobspec;
pub mod lifecycle;
pub mod lint;
pub mod liveness;
pub mod loopguard;
pub mod quarantine;
pub mod ratelimit;
pub mod recovery;
pub mod redact;
pub mod risk;
pub mod router;
pub mod safety;
pub mod scan;
pub mod singleflight;
pub mod stategate;
pub mod targets;
pub mod wire;
pub mod workspace;

pub use agent::{Agent, AgentSelection, AgentSelectionSource, ApiStyle};
pub use approval::ApprovalPolicy;
pub use authority::{AuthorityDecision, AuthorityPolicy, AuthorityTier, Submitter};
pub use constitution::{Constitution, ConstitutionStatus};
pub use cost::JobCost;
pub use deadline::DeadlinePolicy;
pub use error::{CoreError, Result};
pub use events::{DeniedBy, DispatchEvent, EventCategory, EventSink, NullSink, Outcome};
pub use governor::{Admission, Budget, Governor, Spend, SurvivalTier};
pub use lint::{is_structurally_valid, structural_errors, LintError};
pub use loopguard::{fingerprint, LoopGuard, Verdict};
pub use quarantine::{QuarantineLedger, QuarantinePolicy};
pub use ratelimit::{RateDecision, RateLimitPolicy, RateLimiter};
pub use recovery::{FailureKind, RecoveryDirective, RecoveryPolicy, RecoveryVerb, RetryLedger};
pub use redact::{RedactingSink, Redactor};
pub use risk::{RiskBand, RiskLedger, RiskModel, RiskPolicy, RiskScore};
pub use router::{select_route, RouteCandidate};
pub use scan::{scan, Finding, ScanPolicy, ScanReport, Severity};
pub use singleflight::{FlightLease, SingleFlight, SingleFlightDenied, TargetKey};
pub use stategate::{StateGateDecision, StateGatePolicy};
pub use targets::{TargetAllowlist, TargetDecision};
pub use wire::{
    sign_envelope, sign_frame, verify_envelope, verify_frame, Approval, DispatchRequest,
    DispatchResponse, WireError,
};
pub use workspace::{JobWorkspace, NoopWorkspaceProvider, TeardownReport, WorkspaceProvider};
