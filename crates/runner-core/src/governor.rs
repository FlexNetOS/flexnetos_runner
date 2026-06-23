//! Dispatch budget governor — a bounded-autonomy kill-switch (adapted from kclaw0
//! `dark-factory.js::enforceBudget` + `survival.js`).
//!
//! kclaw0's Dark-Factory governance engine runs `immutability → budget → state-machine → holdout`
//! before letting the agent act, and `survival.js` halts the agent when credits run out. The
//! runner-plane analogue is a **hard ceiling on how many jobs an unattended loop may dispatch**
//! before a human/operator must re-arm it — the "owner kill-switch halts autonomy" property
//! (`meta/DARK-FACTORY-RESEARCH.md` §7, Goal G).
//!
//! It is the volume complement of [`crate::loopguard`]: the breaker stops the *same work* looping;
//! the governor stops *runaway total volume*. Both are safety primitives, orthogonal to model
//! routing (weave's domain) — the governor never decides *which* agent runs, only *whether more
//! work may run at all*.
//!
//! **Opt-in and behaviour-preserving:** with no budget set the governor is unlimited, so the
//! default dispatcher is unchanged; an operator arms the ceiling via `FXRUN_DISPATCH_BUDGET`. Pure
//! and in-memory; the long-lived UDS server owns one [`Governor`] across its accept loop.
//!
//! Today the unit is **one dispatch = one unit** (a job-count ceiling). When `atc` reports
//! per-job cost, this generalizes to token/USD budgets without changing the admission shape — see
//! the cost-tracker candidate in `docs/kclaw0-upgrade-ledger.md`.

/// The governor's admission decision for one dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Under budget — dispatch may proceed. Carries spend after this admission.
    Admit { spent: usize, budget: Option<usize> },
    /// Budget exhausted — dispatch refused (kill-switch). Carries the exhausted budget.
    Denied { spent: usize, budget: usize },
}

impl Admission {
    /// Whether dispatch is refused.
    pub fn is_denied(&self) -> bool {
        matches!(self, Admission::Denied { .. })
    }
}

/// A lifetime dispatch budget. `budget == None` is unlimited (the behaviour-preserving default);
/// `Some(n)` admits at most `n` dispatches, then refuses fail-closed until the process is re-armed.
#[derive(Debug, Clone)]
pub struct Governor {
    budget: Option<usize>,
    spent: usize,
}

impl Governor {
    /// An unlimited governor (no ceiling) — the default; admits everything and only counts spend.
    pub fn unlimited() -> Self {
        Self {
            budget: None,
            spent: 0,
        }
    }

    /// A governor that admits at most `max` dispatches over the process lifetime. `max == 0` means
    /// "refuse all" (a hard stop); use [`Governor::unlimited`] for no ceiling.
    pub fn with_budget(max: usize) -> Self {
        Self {
            budget: Some(max),
            spent: 0,
        }
    }

    /// Build from an operator-supplied budget where `0` conventionally means "unlimited"
    /// (the `FXRUN_DISPATCH_BUDGET` convention: unset/`0` → no ceiling).
    pub fn from_env_budget(max: usize) -> Self {
        if max == 0 {
            Self::unlimited()
        } else {
            Self::with_budget(max)
        }
    }

    /// The configured ceiling (`None` = unlimited).
    pub fn budget(&self) -> Option<usize> {
        self.budget
    }

    /// Dispatches admitted so far.
    pub fn spent(&self) -> usize {
        self.spent
    }

    /// Remaining dispatches before the ceiling (`None` = unlimited).
    pub fn remaining(&self) -> Option<usize> {
        self.budget.map(|b| b.saturating_sub(self.spent))
    }

    /// Try to admit one dispatch. On admit, increments spend; on denial, spend is unchanged so a
    /// refused job never consumes budget.
    pub fn admit(&mut self) -> Admission {
        match self.budget {
            Some(b) if self.spent >= b => Admission::Denied {
                spent: self.spent,
                budget: b,
            },
            _ => {
                self.spent += 1;
                Admission::Admit {
                    spent: self.spent,
                    budget: self.budget,
                }
            }
        }
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
    fn unlimited_admits_everything_and_counts_spend() {
        let mut g = Governor::unlimited();
        assert_eq!(g.budget(), None);
        for i in 1..=100 {
            assert!(!g.admit().is_denied());
            assert_eq!(g.spent(), i);
        }
        assert_eq!(g.remaining(), None);
    }

    #[test]
    fn budget_admits_up_to_ceiling_then_denies() {
        let mut g = Governor::with_budget(3);
        assert_eq!(
            g.admit(),
            Admission::Admit {
                spent: 1,
                budget: Some(3)
            }
        );
        assert!(!g.admit().is_denied()); // 2
        assert!(!g.admit().is_denied()); // 3
                                         // 4th is refused — and spend stays at the ceiling (refused work consumes no budget).
        assert_eq!(
            g.admit(),
            Admission::Denied {
                spent: 3,
                budget: 3
            }
        );
        assert_eq!(g.spent(), 3);
        assert_eq!(g.remaining(), Some(0));
        // Stays denied (kill-switch latches until the process is re-armed).
        assert!(g.admit().is_denied());
        assert_eq!(g.spent(), 3);
    }

    #[test]
    fn zero_budget_refuses_all() {
        let mut g = Governor::with_budget(0);
        assert!(g.admit().is_denied());
        assert_eq!(g.spent(), 0);
    }

    #[test]
    fn from_env_treats_zero_as_unlimited() {
        assert_eq!(Governor::from_env_budget(0).budget(), None);
        assert_eq!(Governor::from_env_budget(5).budget(), Some(5));
    }

    #[test]
    fn remaining_tracks_the_ceiling() {
        let mut g = Governor::with_budget(2);
        assert_eq!(g.remaining(), Some(2));
        g.admit();
        assert_eq!(g.remaining(), Some(1));
        g.admit();
        assert_eq!(g.remaining(), Some(0));
    }

    #[test]
    fn default_is_unlimited() {
        assert_eq!(Governor::default().budget(), None);
    }
}
