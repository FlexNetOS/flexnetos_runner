//! Per-job cost — the typed `atc → runner` reporting seam (adapted from kclaw0 `cost-tracker.js`
//! + `dark-factory.js::enforceBudget`, which caps both `usedTokens` and `usedUsd`).
//!
//! The runner is delegate-only: it does not run the model, so it cannot *measure* cost. `atc`
//! (the agent coordinator that actually spawns the model) measures it and **reports it back** when
//! a job completes. This type is that report's payload — what flows from `atc` up through the
//! [`crate::events`] audit log and into the cost-aware [`crate::governor::Governor`].
//!
//! **Fail-open until `atc` fills it.** A kernel that reports no cost (today's `DryRunInvoker`, or
//! any non-agent kernel) yields [`JobCost::ZERO`], so a cost budget that is set simply never charges
//! for unmeasured work — the seam is inert until `atc` provides real numbers, and the existing
//! job-count budget keeps working unchanged.
//!
//! USD is carried as integer **micro-dollars** (1 USD = 1_000_000) to keep the type `Eq`/`Ord` and
//! free of float rounding in the budget comparison; display reconstitutes dollars.

use serde::{Deserialize, Serialize};

/// One job's resource cost, as measured by `atc` and reported back to the runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JobCost {
    /// Total model tokens (prompt + completion) the job consumed.
    pub tokens: u64,
    /// Cost in micro-dollars (USD × 1_000_000) — integer to stay `Eq`/`Ord` and rounding-free.
    pub usd_micros: u64,
}

impl JobCost {
    /// No measured cost — the fail-open default for kernels that don't report (the seam is inert).
    pub const ZERO: JobCost = JobCost {
        tokens: 0,
        usd_micros: 0,
    };

    /// Construct from tokens and a micro-dollar amount.
    pub fn new(tokens: u64, usd_micros: u64) -> Self {
        Self { tokens, usd_micros }
    }

    /// Whether this report carries any measured cost.
    pub fn is_measured(&self) -> bool {
        self.tokens != 0 || self.usd_micros != 0
    }

    /// Saturating component-wise sum (accumulating spend never overflows/panics).
    pub fn saturating_add(self, other: JobCost) -> JobCost {
        JobCost {
            tokens: self.tokens.saturating_add(other.tokens),
            usd_micros: self.usd_micros.saturating_add(other.usd_micros),
        }
    }

    /// The USD value as a float, for display/logging only (never for the budget comparison).
    pub fn usd(&self) -> f64 {
        self.usd_micros as f64 / 1_000_000.0
    }
}

impl std::fmt::Display for JobCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} tok / ${:.4}", self.tokens, self.usd())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_unmeasured_and_inert() {
        assert!(!JobCost::ZERO.is_measured());
        assert_eq!(JobCost::ZERO.tokens, 0);
        assert_eq!(JobCost::ZERO.usd_micros, 0);
    }

    #[test]
    fn measured_detects_either_dimension() {
        assert!(JobCost::new(10, 0).is_measured());
        assert!(JobCost::new(0, 5).is_measured());
        assert!(!JobCost::new(0, 0).is_measured());
    }

    #[test]
    fn saturating_add_accumulates_without_overflow() {
        let a = JobCost::new(100, 2_000);
        let b = JobCost::new(50, 3_000);
        assert_eq!(a.saturating_add(b), JobCost::new(150, 5_000));
        // No panic at the ceiling.
        let big = JobCost::new(u64::MAX, u64::MAX);
        assert_eq!(big.saturating_add(a), JobCost::new(u64::MAX, u64::MAX));
    }

    #[test]
    fn usd_micros_reconstitutes_dollars_for_display() {
        let c = JobCost::new(1234, 2_500_000); // $2.50
        assert!((c.usd() - 2.5).abs() < 1e-9);
        assert_eq!(c.to_string(), "1234 tok / $2.5000");
    }

    #[test]
    fn json_roundtrips() {
        let c = JobCost::new(42, 99);
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<JobCost>(&s).unwrap(), c);
    }
}
