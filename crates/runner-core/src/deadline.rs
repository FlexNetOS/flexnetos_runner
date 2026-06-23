//! Per-job wall-clock deadline policy (adapted from the same mechanism surfaced *independently* by
//! three prior-art sources in the cycle-12 deep-research sweep: `kclaw0`'s `docker-exec.js`
//! `defaultTimeout` + `dockerStop` kill path, `coleam00/Archon`'s `GIT_OPERATION_TIMEOUT_MS` per-op
//! ceilings, and `strongdm/attractor`'s `timeout` node attribute — *"the engine may interrupt
//! handlers exceeding it"*).
//!
//! It is the one failure axis the runner's existing guards do **not** cover:
//! - the **loop breaker** ([`crate::loopguard`]) catches the same work dispatched *in a loop*;
//! - the **governor** ([`crate::governor`]) caps aggregate *cost/volume*;
//! - **quarantine** ([`crate::quarantine`]) latches a job that *keeps failing the same way*.
//!
//! None of them bounds a *single, non-looping, in-budget* job that simply **hangs** — never
//! returning, burning a worker (and, in P3, a real kernel subprocess) indefinitely. The deadline is
//! that bound: a wall-clock ceiling on one delegation, after which the runner stops waiting, reclaims
//! the workspace (the [`crate::workspace`] teardown guarantee), and routes the timeout through
//! recovery + quarantine like any other failure.
//!
//! **Two sources, fail-closed `min`.** A deadline can come from the App (a per-job
//! [`crate::wire::DispatchRequest::deadline_secs`] on the request envelope — like the approval grant,
//! an out-of-band fact rather than a field of the signed spec) and from the operator (a runner-wide
//! ceiling, `FXRUN_DEFAULT_DEADLINE_SECS`). The *effective* deadline is the **tighter** of whichever
//! are set ([`DeadlinePolicy::effective`]), so the App can ask for less time but **never more** than
//! the operator's cap, and a job that names no deadline still inherits the cap. Both unset → no
//! deadline (behaviour-preserving default; the watchdog is not even engaged).
//!
//! Putting the per-job request on the *envelope* (not the signed `JobSpec`) keeps the safety property
//! intact: because the effective value is `min(cap, requested)`, a tampered request can only ever
//! *shorten* a job, never extend it past the operator ceiling — so on the local UDS socket the worst
//! a tamper achieves is a bounded availability nuisance (a too-early timeout → recovery retry), not a
//! guardrail bypass. The operator ceiling — the actual bound — is server-side config, untamperable.
//!
//! `runner-core` stays clock-free: this module only *computes* the effective ceiling (a `Duration`).
//! The actual wall-clock measurement / watchdog lives in the `runner-dispatch` binary (which may use
//! `std::time`). Delegate-only: the deadline is a dispatch-governance bound the runner observes and
//! acts on (abandon + reclaim + signal); the kernel still owns *how* the work runs — and in P3 the
//! real invoker additionally hard-kills its own subprocess at the deadline (attractor's "interrupt").
//! Orthogonal to model routing (weave's domain).

use std::time::Duration;

/// The runner-wide deadline ceiling an operator configures. `None` = no operator cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeadlinePolicy {
    default_secs: Option<u64>,
}

impl DeadlinePolicy {
    /// A policy with an operator-wide ceiling of `secs` (a `Some(0)` is treated as *no* cap, since a
    /// zero-second deadline could never be met — see [`Self::from_secs`]).
    pub fn new(default_secs: Option<u64>) -> Self {
        Self {
            default_secs: default_secs.filter(|s| *s > 0),
        }
    }

    /// Build from a raw seconds value where `0` means "no cap" (the env-var convention used by the
    /// other budget knobs): `0` → disabled, `n` → an `n`-second ceiling.
    pub fn from_secs(secs: u64) -> Self {
        Self::new(Some(secs))
    }

    /// The behaviour-preserving default: no operator ceiling.
    pub fn disabled() -> Self {
        Self { default_secs: None }
    }

    /// Whether an operator ceiling is configured.
    pub fn is_active(&self) -> bool {
        self.default_secs.is_some()
    }

    /// The configured operator ceiling in seconds (if any).
    pub fn default_secs(&self) -> Option<u64> {
        self.default_secs
    }

    /// The **effective** deadline for a job that requested `job_requested` seconds: the tighter of the
    /// operator ceiling and the job's request (each treating `0`/`None` as "unset"). Returns the
    /// duration to wait, or `None` when neither side sets a bound (the watchdog stays disengaged).
    ///
    /// - both set → `min` (the App may ask for *less*, never *more*, than the operator cap);
    /// - only one set → that one;
    /// - neither → `None`.
    pub fn effective(&self, job_requested: Option<u64>) -> Option<Duration> {
        let requested = job_requested.filter(|s| *s > 0);
        let secs = match (self.default_secs, requested) {
            (Some(cap), Some(req)) => Some(cap.min(req)),
            (Some(cap), None) => Some(cap),
            (None, Some(req)) => Some(req),
            (None, None) => None,
        };
        secs.map(Duration::from_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_policy_has_no_effective_deadline_without_a_request() {
        let p = DeadlinePolicy::disabled();
        assert!(!p.is_active());
        assert_eq!(p.effective(None), None);
    }

    #[test]
    fn job_request_applies_even_without_an_operator_cap() {
        let p = DeadlinePolicy::disabled();
        assert_eq!(p.effective(Some(30)), Some(Duration::from_secs(30)));
    }

    #[test]
    fn operator_cap_applies_even_without_a_job_request() {
        let p = DeadlinePolicy::from_secs(120);
        assert_eq!(p.effective(None), Some(Duration::from_secs(120)));
    }

    #[test]
    fn effective_is_the_tighter_of_the_two() {
        let p = DeadlinePolicy::from_secs(120);
        // The job asks for less → the job's request wins.
        assert_eq!(p.effective(Some(30)), Some(Duration::from_secs(30)));
        // The job asks for MORE than the cap → the cap wins (can't exceed the operator ceiling).
        assert_eq!(p.effective(Some(600)), Some(Duration::from_secs(120)));
    }

    #[test]
    fn zero_is_treated_as_unset_on_both_sides() {
        assert_eq!(DeadlinePolicy::from_secs(0).default_secs(), None);
        assert_eq!(DeadlinePolicy::from_secs(0).effective(None), None);
        assert_eq!(DeadlinePolicy::disabled().effective(Some(0)), None);
        // A zero operator cap with a real job request → the job request still applies.
        assert_eq!(
            DeadlinePolicy::from_secs(0).effective(Some(45)),
            Some(Duration::from_secs(45))
        );
    }
}
