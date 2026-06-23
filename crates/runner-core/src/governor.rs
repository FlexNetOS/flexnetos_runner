//! Dispatch budget governor — a bounded-autonomy kill-switch (adapted from kclaw0
//! `dark-factory.js::enforceBudget` + `survival.js`).
//!
//! kclaw0's Dark-Factory engine refuses to act once `usedTokens > maxTokens` **or**
//! `usedUsd > maxUsd`, and `survival.js` halts at zero credits. This is the runner-plane analogue:
//! a hard ceiling on how much an unattended loop may dispatch before a human re-arms it — the
//! "owner kill-switch halts autonomy" property (`meta/DARK-FACTORY-RESEARCH.md` §7, Goal G).
//!
//! It is multi-dimensional: a **job count**, a **token** total, and a **USD** total may each be
//! capped independently. The job count is known at admission; tokens/USD are only known *after*
//! `atc` reports a job's [`JobCost`], so the governor has two moves:
//! - [`Governor::admit`] — the pre-dispatch gate: deny if any capped dimension is **already** at or
//!   over its limit; otherwise admit and reserve one job.
//! - [`Governor::charge`] — post-dispatch: add the cost `atc` reported, so the *next* `admit` sees
//!   it. This is exactly kclaw0's "used vs max" check, applied between jobs.
//!
//! **Behaviour-preserving + fail-open:** with no caps set it is unlimited; an unmeasured job
//! ([`JobCost::ZERO`], today's default) charges nothing, so a cost cap is inert until `atc` reports
//! real numbers — the existing job-count budget keeps working unchanged. A safety primitive,
//! orthogonal to model routing (weave's domain): the governor never decides *which* agent runs,
//! only *whether more work may run at all*.

use crate::cost::JobCost;

/// The governor's admission decision for one dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Under every cap — dispatch may proceed (one job reserved).
    Admit,
    /// A cap is already met — dispatch refused (kill-switch). Carries the exhausted-dimension reason.
    Denied { reason: String },
}

impl Admission {
    /// Whether dispatch is refused.
    pub fn is_denied(&self) -> bool {
        matches!(self, Admission::Denied { .. })
    }
}

/// Per-dimension ceilings. `None` = that dimension is uncapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budget {
    pub jobs: Option<usize>,
    pub tokens: Option<u64>,
    pub usd_micros: Option<u64>,
}

impl Budget {
    /// Whether any dimension is capped (i.e. the governor will ever deny).
    pub fn is_capped(&self) -> bool {
        self.jobs.is_some() || self.tokens.is_some() || self.usd_micros.is_some()
    }
}

/// Accumulated spend across the process lifetime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spend {
    pub jobs: usize,
    pub cost: JobCost,
}

/// A lifetime, multi-dimensional dispatch budget. Unlimited unless a dimension is capped; capped
/// dimensions refuse fail-closed once met, latching until the process is re-armed.
#[derive(Debug, Clone)]
pub struct Governor {
    budget: Budget,
    spent: Spend,
}

impl Governor {
    /// An unlimited governor (no ceiling) — the behaviour-preserving default.
    pub fn unlimited() -> Self {
        Self {
            budget: Budget::default(),
            spent: Spend::default(),
        }
    }

    /// A governor capping only the job count (the cycle-2 behaviour). `0` jobs means "refuse all".
    pub fn with_jobs(max: usize) -> Self {
        Self {
            budget: Budget {
                jobs: Some(max),
                ..Budget::default()
            },
            spent: Spend::default(),
        }
    }

    /// A governor with an explicit multi-dimensional [`Budget`].
    pub fn with_budget(budget: Budget) -> Self {
        Self {
            budget,
            spent: Spend::default(),
        }
    }

    /// Build from operator env values where `0` conventionally means "uncapped" for that dimension
    /// (`FXRUN_DISPATCH_BUDGET` jobs, `FXRUN_TOKEN_BUDGET` tokens, `FXRUN_USD_MICROS_BUDGET` USD-µ).
    pub fn from_env(jobs: usize, tokens: u64, usd_micros: u64) -> Self {
        let opt_usize = |n: usize| (n != 0).then_some(n);
        let opt_u64 = |n: u64| (n != 0).then_some(n);
        Self::with_budget(Budget {
            jobs: opt_usize(jobs),
            tokens: opt_u64(tokens),
            usd_micros: opt_u64(usd_micros),
        })
    }

    /// The configured ceilings.
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// Accumulated spend so far.
    pub fn spent(&self) -> Spend {
        self.spent
    }

    /// The pre-dispatch gate. Denies if any capped dimension is already at/over its limit (with a
    /// reason naming the exhausted dimension); otherwise reserves one job and admits.
    pub fn admit(&mut self) -> Admission {
        if let Some(max) = self.budget.jobs {
            if self.spent.jobs >= max {
                return Admission::Denied {
                    reason: format!(
                        "job budget exhausted: {}/{max} jobs this session",
                        self.spent.jobs
                    ),
                };
            }
        }
        if let Some(max) = self.budget.tokens {
            if self.spent.cost.tokens >= max {
                return Admission::Denied {
                    reason: format!(
                        "token budget exhausted: {}/{max} tokens this session",
                        self.spent.cost.tokens
                    ),
                };
            }
        }
        if let Some(max) = self.budget.usd_micros {
            if self.spent.cost.usd_micros >= max {
                return Admission::Denied {
                    reason: format!(
                        "USD budget exhausted: ${:.4}/${:.4} this session",
                        self.spent.cost.usd(),
                        JobCost::new(0, max).usd()
                    ),
                };
            }
        }
        self.spent.jobs += 1;
        Admission::Admit
    }

    /// Post-dispatch: add the cost `atc` reported for a completed job, so the next [`admit`](Self::admit)
    /// sees it. Unmeasured cost ([`JobCost::ZERO`]) is a harmless no-op (the fail-open seam).
    pub fn charge(&mut self, cost: JobCost) {
        self.spent.cost = self.spent.cost.saturating_add(cost);
    }
}

impl Default for Governor {
    /// Unlimited — the dispatcher only enforces a ceiling when an operator sets one.
    fn default() -> Self {
        Self::unlimited()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_admits_everything() {
        let mut g = Governor::unlimited();
        assert!(!g.budget().is_capped());
        for _ in 0..100 {
            assert_eq!(g.admit(), Admission::Admit);
        }
        assert_eq!(g.spent().jobs, 100);
    }

    #[test]
    fn job_cap_admits_up_to_ceiling_then_denies_and_latches() {
        let mut g = Governor::with_jobs(2);
        assert_eq!(g.admit(), Admission::Admit);
        assert_eq!(g.admit(), Admission::Admit);
        let denied = g.admit();
        assert!(denied.is_denied());
        if let Admission::Denied { reason } = denied {
            assert!(reason.contains("job budget exhausted"));
        }
        // Latches; spend does not advance past the ceiling on denial.
        assert!(g.admit().is_denied());
        assert_eq!(g.spent().jobs, 2);
    }

    #[test]
    fn token_cap_is_enforced_between_jobs_via_charge() {
        // Cap 1000 tokens; the job count is uncapped.
        let mut g = Governor::with_budget(Budget {
            tokens: Some(1000),
            ..Budget::default()
        });
        assert_eq!(g.admit(), Admission::Admit); // first job admitted (no spend yet)
        g.charge(JobCost::new(900, 0)); // atc reports 900 tokens
        assert_eq!(g.admit(), Admission::Admit); // still under 1000
        g.charge(JobCost::new(200, 0)); // now 1100 total
        let denied = g.admit();
        assert!(denied.is_denied());
        if let Admission::Denied { reason } = denied {
            assert!(reason.contains("token budget exhausted"));
        }
    }

    #[test]
    fn usd_cap_is_enforced_and_reported_in_dollars() {
        // $0.50 cap.
        let mut g = Governor::with_budget(Budget {
            usd_micros: Some(500_000),
            ..Budget::default()
        });
        assert_eq!(g.admit(), Admission::Admit);
        g.charge(JobCost::new(0, 600_000)); // $0.60 spent
        match g.admit() {
            Admission::Denied { reason } => {
                assert!(reason.contains("USD budget exhausted"));
                assert!(reason.contains("0.6000")); // spent shown in dollars
                assert!(reason.contains("0.5000")); // cap shown in dollars
            }
            Admission::Admit => panic!("should be denied past the USD cap"),
        }
    }

    #[test]
    fn unmeasured_cost_never_charges_a_cost_cap() {
        // A token cap with only ZERO-cost jobs (today's DryRunInvoker) → never trips on cost.
        let mut g = Governor::with_budget(Budget {
            tokens: Some(10),
            ..Budget::default()
        });
        for _ in 0..50 {
            assert_eq!(g.admit(), Admission::Admit);
            g.charge(JobCost::ZERO); // fail-open: unmeasured work charges nothing
        }
        assert_eq!(g.spent().cost, JobCost::ZERO);
    }

    #[test]
    fn from_env_treats_zero_as_uncapped_per_dimension() {
        assert!(!Governor::from_env(0, 0, 0).budget().is_capped());
        assert_eq!(Governor::from_env(5, 0, 0).budget().jobs, Some(5));
        assert_eq!(Governor::from_env(0, 1000, 0).budget().tokens, Some(1000));
        assert_eq!(
            Governor::from_env(0, 0, 250_000).budget().usd_micros,
            Some(250_000)
        );
    }

    #[test]
    fn default_is_unlimited() {
        assert!(!Governor::default().budget().is_capped());
    }
}
