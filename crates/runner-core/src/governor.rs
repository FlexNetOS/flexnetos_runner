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
//!
//! ## Survival tiers + debounced halt (adapted from `Conway-Research/automaton`)
//! automaton runs a graduated **balance ladder** (healthy → conserving → critical → distress →
//! dead) with a grace window before it declares itself dead, instead of a single hard cliff at
//! zero. The runner takes the runner-plane half of that:
//! - [`SurvivalTier`] — a read-only classification of how close the *worst* capped dimension is to
//!   its ceiling ([`Full`](SurvivalTier::Full) → [`Conserving`](SurvivalTier::Conserving) →
//!   [`Distress`](SurvivalTier::Distress) → [`Halted`](SurvivalTier::Halted)). It is **observability**
//!   the operator / weave can act on *before* the wall (e.g. stop queuing low-priority work); the
//!   runner itself still only hard-stops at the cap. The *model-downgrade* action automaton takes in
//!   its lower tiers stays weave's job — the runner exposes the tier, weave decides the response.
//! - a **debounced floor** ([`Budget::grace`]): when a cap is first met, allow up to `grace` further
//!   "distress" admits before refusing *all* dispatch, so a single tiny overshoot (or an already
//!   in-flight job) doesn't latch the kill-switch on a boundary. Default `grace = 0` reproduces the
//!   strict cliff exactly (behaviour-preserving); a non-zero grace is a deliberate, bounded opt-in.

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

/// How close the *worst* capped dimension is to its ceiling — a graduated degradation signal
/// (automaton's balance ladder). Read-only: the runner exposes it; weave/the operator decides the
/// response (e.g. shed low-priority work). The runner itself only hard-stops at [`Halted`](Self::Halted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SurvivalTier {
    /// Below 75% of every cap (or uncapped) — full autonomy.
    Full,
    /// ≥ 75% of some cap — getting close; a good point to shed non-essential work.
    Conserving,
    /// ≥ 90% of some cap — nearly exhausted; essential work only.
    Distress,
    /// A cap is met or exceeded — at the wall (dispatch halts once any grace is consumed).
    Halted,
}

impl SurvivalTier {
    /// Classify a worst-dimension usage fraction (`used / cap`; ≥ 1.0 means at/over the cap).
    pub fn from_fraction(fraction: f64) -> Self {
        if fraction >= 1.0 {
            SurvivalTier::Halted
        } else if fraction >= 0.9 {
            SurvivalTier::Distress
        } else if fraction >= 0.75 {
            SurvivalTier::Conserving
        } else {
            SurvivalTier::Full
        }
    }

    /// Whether the runner is operating below full autonomy (worth surfacing to the operator).
    pub fn is_degraded(&self) -> bool {
        *self != SurvivalTier::Full
    }

    /// A short lowercase label for banners / audit details.
    pub fn label(&self) -> &'static str {
        match self {
            SurvivalTier::Full => "full",
            SurvivalTier::Conserving => "conserving",
            SurvivalTier::Distress => "distress",
            SurvivalTier::Halted => "halted",
        }
    }
}

impl std::fmt::Display for SurvivalTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Per-dimension ceilings. `None` = that dimension is uncapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budget {
    pub jobs: Option<usize>,
    pub tokens: Option<u64>,
    pub usd_micros: Option<u64>,
    /// Debounced floor: how many admits are allowed *past* a met cap before dispatch hard-halts.
    /// `0` (the default) is the strict cliff — deny the moment a cap is met (behaviour-preserving).
    pub grace: usize,
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
    /// Distress admits granted past a met cap (counts against [`Budget::grace`]).
    pub grace_used: usize,
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
    /// `grace` is the debounced floor (`FXRUN_BUDGET_GRACE`; `0` = strict cliff, the default).
    pub fn from_env(jobs: usize, tokens: u64, usd_micros: u64, grace: usize) -> Self {
        let opt_usize = |n: usize| (n != 0).then_some(n);
        let opt_u64 = |n: u64| (n != 0).then_some(n);
        Self::with_budget(Budget {
            jobs: opt_usize(jobs),
            tokens: opt_u64(tokens),
            usd_micros: opt_u64(usd_micros),
            grace,
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

    /// The reason a capped dimension is already met/exceeded, if any (first dimension checked:
    /// jobs → tokens → USD). `None` means every capped dimension is still under its ceiling.
    fn exhausted_reason(&self) -> Option<String> {
        if let Some(max) = self.budget.jobs {
            if self.spent.jobs >= max {
                return Some(format!(
                    "job budget exhausted: {}/{max} jobs this session",
                    self.spent.jobs
                ));
            }
        }
        if let Some(max) = self.budget.tokens {
            if self.spent.cost.tokens >= max {
                return Some(format!(
                    "token budget exhausted: {}/{max} tokens this session",
                    self.spent.cost.tokens
                ));
            }
        }
        if let Some(max) = self.budget.usd_micros {
            if self.spent.cost.usd_micros >= max {
                return Some(format!(
                    "USD budget exhausted: ${:.4}/${:.4} this session",
                    self.spent.cost.usd(),
                    JobCost::new(0, max).usd()
                ));
            }
        }
        None
    }

    /// The pre-dispatch gate. If a capped dimension is already met, the **debounced floor** allows up
    /// to [`Budget::grace`] further distress admits before refusing (default `grace = 0` → deny at
    /// once, the strict cliff). Otherwise reserves one job and admits.
    pub fn admit(&mut self) -> Admission {
        if let Some(reason) = self.exhausted_reason() {
            // A cap is met. Grant a bounded number of distress admits past the line before halting,
            // so a single tiny overshoot / in-flight job doesn't latch the kill-switch on a boundary.
            if self.spent.grace_used < self.budget.grace {
                self.spent.grace_used += 1;
                self.spent.jobs += 1;
                return Admission::Admit;
            }
            return Admission::Denied { reason };
        }
        self.spent.jobs += 1;
        Admission::Admit
    }

    /// The current [`SurvivalTier`] — how close the worst capped dimension is to its ceiling. Read-
    /// only; the runner exposes it for the operator / weave to act on (the runner itself only hard-
    /// stops at the cap, modulo the grace floor).
    pub fn tier(&self) -> SurvivalTier {
        let frac = |used: f64, cap: Option<u64>| match cap {
            Some(c) if c > 0 => used / c as f64,
            _ => 0.0,
        };
        let worst = frac(self.spent.jobs as f64, self.budget.jobs.map(|j| j as u64))
            .max(frac(self.spent.cost.tokens as f64, self.budget.tokens))
            .max(frac(
                self.spent.cost.usd_micros as f64,
                self.budget.usd_micros,
            ));
        SurvivalTier::from_fraction(worst)
    }

    /// The configured debounced-floor grace (admits allowed past a met cap before halting).
    pub fn grace(&self) -> usize {
        self.budget.grace
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
        assert!(!Governor::from_env(0, 0, 0, 0).budget().is_capped());
        assert_eq!(Governor::from_env(5, 0, 0, 0).budget().jobs, Some(5));
        assert_eq!(
            Governor::from_env(0, 1000, 0, 0).budget().tokens,
            Some(1000)
        );
        assert_eq!(
            Governor::from_env(0, 0, 250_000, 0).budget().usd_micros,
            Some(250_000)
        );
    }

    #[test]
    fn from_env_carries_the_grace_floor() {
        assert_eq!(Governor::from_env(5, 0, 0, 3).grace(), 3);
        assert_eq!(Governor::from_env(0, 0, 0, 0).grace(), 0);
    }

    #[test]
    fn default_is_unlimited() {
        assert!(!Governor::default().budget().is_capped());
    }

    #[test]
    fn tier_climbs_as_a_capped_dimension_fills() {
        // 10-job cap. Tier reflects the worst dimension's used/cap fraction.
        let mut g = Governor::with_jobs(10);
        assert_eq!(g.tier(), SurvivalTier::Full); // 0/10
        for _ in 0..8 {
            g.admit();
        }
        assert_eq!(g.tier(), SurvivalTier::Conserving); // 8/10 = 0.80 ≥ 0.75
        g.admit();
        assert_eq!(g.tier(), SurvivalTier::Distress); // 9/10 = 0.90 ≥ 0.9
        g.admit();
        assert_eq!(g.tier(), SurvivalTier::Halted); // 10/10 = 1.0 ≥ 1.0
    }

    #[test]
    fn uncapped_governor_is_always_full_tier() {
        let mut g = Governor::unlimited();
        for _ in 0..1000 {
            g.admit();
        }
        assert_eq!(g.tier(), SurvivalTier::Full);
        assert!(!g.tier().is_degraded());
    }

    #[test]
    fn tier_tracks_the_worst_dimension() {
        // Jobs barely used, but tokens nearly exhausted → tier follows tokens.
        let mut g = Governor::with_budget(Budget {
            jobs: Some(100),
            tokens: Some(1000),
            ..Budget::default()
        });
        g.admit();
        g.charge(JobCost::new(950, 0)); // 95% of tokens
        assert_eq!(g.tier(), SurvivalTier::Distress);
    }

    #[test]
    fn grace_floor_allows_bounded_distress_admits_then_halts() {
        // 2-job cap with a grace of 2: admits 2 (under cap) + 2 (distress grace) = 4, then denies.
        let mut g = Governor::with_budget(Budget {
            jobs: Some(2),
            grace: 2,
            ..Budget::default()
        });
        assert_eq!(g.admit(), Admission::Admit); // 1/2
        assert_eq!(g.admit(), Admission::Admit); // 2/2
        assert_eq!(g.admit(), Admission::Admit); // grace 1
        assert_eq!(g.admit(), Admission::Admit); // grace 2
        assert!(g.admit().is_denied()); // grace exhausted → halt
        assert!(g.admit().is_denied()); // latches
        assert_eq!(g.spent().grace_used, 2);
        assert_eq!(g.tier(), SurvivalTier::Halted);
    }

    #[test]
    fn zero_grace_is_the_strict_cliff() {
        // Default grace 0 reproduces the exact pre-existing deny-at-cap behaviour.
        let mut g = Governor::with_jobs(1);
        assert_eq!(g.admit(), Admission::Admit);
        assert!(g.admit().is_denied());
        assert_eq!(g.spent().grace_used, 0);
    }

    #[test]
    fn survival_tier_ordering_and_labels() {
        assert!(SurvivalTier::Full < SurvivalTier::Halted);
        assert!(SurvivalTier::Distress > SurvivalTier::Conserving);
        assert_eq!(SurvivalTier::from_fraction(0.0), SurvivalTier::Full);
        assert_eq!(SurvivalTier::from_fraction(0.8), SurvivalTier::Conserving);
        assert_eq!(SurvivalTier::from_fraction(0.95), SurvivalTier::Distress);
        assert_eq!(SurvivalTier::from_fraction(1.5), SurvivalTier::Halted);
        assert_eq!(SurvivalTier::Distress.to_string(), "distress");
    }
}
