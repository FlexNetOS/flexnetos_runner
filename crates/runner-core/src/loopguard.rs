//! Loop-detection circuit breaker (ADR-0008 safety; adapted from kclaw0 `loop-detection.js`).
//!
//! The runner is the dispatch choke point of an autonomous loop, so a runaway agent loop shows up
//! here as **the same semantic job dispatched over and over**. kclaw0's `loop-detection.js` trips on
//! "4 identical tool calls in a row"; this is the runner-plane analogue: trip when one job
//! *fingerprint* recurs `trip_threshold` times within the last `window` dispatches.
//!
//! This is the documented #1 failure mode of unattended loops — infinite retries burning cost
//! (see `meta/DARK-FACTORY-RESEARCH.md` §5; §7 implication #4 "circuit breakers"). It is a *safety*
//! primitive, orthogonal to model routing (which weave owns): the breaker never decides *which*
//! agent runs, only that the *same work* must not be dispatched in a tight loop. Fail-closed: a
//! tripped job is rejected at the dispatch boundary, exactly like fork-PR isolation.
//!
//! Pure and in-memory — no I/O, no clock. The long-lived UDS server owns one [`LoopGuard`] across
//! its (single-threaded) accept loop; recovery is automatic as unrelated jobs age the offending
//! fingerprint out of the window.

use crate::jobspec::JobSpec;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

/// Stable fingerprint of a job's **semantics** — its [`crate::jobspec::JobKind`] (repo / head_sha /
/// kind / pr_number / agent) — deliberately **excluding** the volatile `id` and `correlation_id`.
///
/// Two independent deliveries of the *same work* therefore share a fingerprint (which is what lets
/// the breaker see a loop), while the unique `id` still serves as the per-delivery dedup key
/// elsewhere. kclaw0 keys its detector on `(tool, args)` — the semantic action, not the call id;
/// this is the same choice.
pub fn fingerprint(job: &JobSpec) -> String {
    let bytes = serde_json::to_vec(&job.job).expect("JobKind serializes");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hex::encode(hasher.finalize())
}

/// The breaker's decision for one observed dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Below threshold — dispatch may proceed.
    Pass,
    /// Tripped — this fingerprint hit `trip_threshold` within the window. `count` is its current
    /// occurrence count in the window (≥ threshold).
    Trip { count: usize },
}

impl Verdict {
    /// Whether dispatch must be refused.
    pub fn is_tripped(&self) -> bool {
        matches!(self, Verdict::Trip { .. })
    }
}

/// A bounded recent-dispatch history that trips when one job fingerprint recurs `trip_threshold`
/// times within the last `window` observations. The single source of truth for the runner's
/// loop circuit breaker.
#[derive(Debug, Clone)]
pub struct LoopGuard {
    window: usize,
    trip_threshold: usize,
    recent: VecDeque<String>,
}

impl LoopGuard {
    /// Build a guard. `trip_threshold` is clamped to `1..=window` (a threshold above the window
    /// could never trip; zero is meaningless).
    pub fn new(window: usize, trip_threshold: usize) -> Self {
        let window = window.max(1);
        let trip_threshold = trip_threshold.clamp(1, window);
        Self {
            window,
            trip_threshold,
            recent: VecDeque::with_capacity(window),
        }
    }

    /// The configured window size (number of recent dispatches retained).
    pub fn window(&self) -> usize {
        self.window
    }

    /// The configured trip threshold (identical-fingerprint count that trips the breaker).
    pub fn trip_threshold(&self) -> usize {
        self.trip_threshold
    }

    /// Observe a job about to be dispatched and decide whether to allow it. Records the dispatch
    /// (evicting the oldest beyond `window`), then trips if this fingerprint now occurs at least
    /// `trip_threshold` times within the window.
    pub fn observe(&mut self, job: &JobSpec) -> Verdict {
        let fp = fingerprint(job);
        self.recent.push_back(fp.clone());
        if self.recent.len() > self.window {
            self.recent.pop_front();
        }
        let count = self.recent.iter().filter(|f| **f == fp).count();
        if count >= self.trip_threshold {
            Verdict::Trip { count }
        } else {
            Verdict::Pass
        }
    }
}

impl Default for LoopGuard {
    /// kclaw0's `loop-detection.js` defaults: 4 identical within a window of 8.
    fn default() -> Self {
        Self::new(8, 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Agent;
    use crate::jobspec::{JobKind, JobSpec};

    fn ci(id: &str, sha: &str) -> JobSpec {
        JobSpec {
            id: id.into(),
            correlation_id: format!("corr-{id}"),
            from_fork: false,
            job: JobKind::Ci {
                repo: "FlexNetOS/x".into(),
                head_sha: sha.into(),
            },
        }
    }

    #[test]
    fn fingerprint_ignores_volatile_id_but_tracks_semantics() {
        // Same work, different delivery id/correlation → same fingerprint (a loop is visible).
        assert_eq!(
            fingerprint(&ci("job-1", "abc")),
            fingerprint(&ci("job-2", "abc"))
        );
        // Different work (different sha) → different fingerprint.
        assert_ne!(
            fingerprint(&ci("job-1", "abc")),
            fingerprint(&ci("job-1", "def"))
        );
    }

    #[test]
    fn fingerprint_distinguishes_agent_backend() {
        let review = |agent| JobSpec {
            id: "j".into(),
            correlation_id: "c".into(),
            from_fork: false,
            job: JobKind::ReviewGate {
                repo: "r".into(),
                pr_number: 1,
                head_sha: "s".into(),
                agent,
            },
        };
        assert_ne!(
            fingerprint(&review(Agent::Claude)),
            fingerprint(&review(Agent::Codex))
        );
    }

    #[test]
    fn trips_after_threshold_identical_dispatches() {
        let mut g = LoopGuard::new(8, 4);
        // First three identical dispatches pass.
        assert_eq!(g.observe(&ci("a", "abc")), Verdict::Pass);
        assert_eq!(g.observe(&ci("b", "abc")), Verdict::Pass);
        assert_eq!(g.observe(&ci("c", "abc")), Verdict::Pass);
        // The 4th identical (same semantics, distinct ids) trips.
        assert_eq!(g.observe(&ci("d", "abc")), Verdict::Trip { count: 4 });
    }

    #[test]
    fn distinct_work_never_trips() {
        let mut g = LoopGuard::new(8, 4);
        for i in 0..20 {
            // Each job is distinct work → no fingerprint ever repeats.
            assert_eq!(
                g.observe(&ci(&format!("j{i}"), &format!("sha{i}"))),
                Verdict::Pass
            );
        }
    }

    #[test]
    fn recovers_when_offending_fingerprint_ages_out_of_window() {
        let mut g = LoopGuard::new(4, 4);
        // Trip on 4 identical.
        for id in ["a", "b", "c"] {
            assert_eq!(g.observe(&ci(id, "loop")), Verdict::Pass);
        }
        assert!(g.observe(&ci("d", "loop")).is_tripped());
        // Four unrelated dispatches push the offending fingerprint fully out of the window…
        for i in 0..4 {
            g.observe(&ci(&format!("u{i}"), &format!("other{i}")));
        }
        // …so the work is allowed again (natural recovery, no manual reset).
        assert_eq!(g.observe(&ci("e", "loop")), Verdict::Pass);
    }

    #[test]
    fn threshold_is_clamped_into_window() {
        // Threshold above window can never trip → clamped to window.
        let g = LoopGuard::new(4, 99);
        assert_eq!(g.trip_threshold(), 4);
        // Zero threshold is meaningless → clamped to 1.
        let g = LoopGuard::new(4, 0);
        assert_eq!(g.trip_threshold(), 1);
    }

    #[test]
    fn default_matches_kclaw0_four_in_eight() {
        let g = LoopGuard::default();
        assert_eq!(g.window(), 8);
        assert_eq!(g.trip_threshold(), 4);
    }
}
