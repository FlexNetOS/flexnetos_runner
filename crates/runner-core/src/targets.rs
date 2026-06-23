//! Delegation-target allowlist.
//!
//! The runner is delegate-only, but it is still the choke point for **which kernel endpoint** may be
//! reached. This policy is the runner-plane analogue of fail-closed domain / egress allowlists from
//! automaton + attractor: once configured, only named kernels are reachable. It governs kernel
//! reachability (`loop` / `atc` / `hf` / `weave`), not model/provider selection (weave owns that).
//!
//! Behaviour-preserving default: when the operator does not configure an allowlist, all existing
//! routes are allowed. If the operator configures the variable but leaves it empty, the policy is
//! active with an empty set and therefore denies every kernel fail-closed.

use crate::router::Kernel;
use std::collections::HashSet;

/// Kernel reachability policy. `allowed = None` means disabled/inert; `Some(empty)` means active and
/// fail-closed deny-all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetAllowlist {
    allowed: Option<HashSet<Kernel>>,
}

impl TargetAllowlist {
    /// Inert policy: preserve existing routing behavior.
    pub fn disabled() -> Self {
        Self { allowed: None }
    }

    /// Active policy with the exact allowed kernel set. An empty iterator is a deny-all policy.
    pub fn only(kernels: impl IntoIterator<Item = Kernel>) -> Self {
        Self {
            allowed: Some(kernels.into_iter().collect()),
        }
    }

    /// Parse an optional comma-separated env value. `None` disables the gate; `Some("")` enables a
    /// fail-closed empty allowlist; `all`/`*` allows all canonical kernels.
    pub fn from_env(value: Option<&str>) -> Result<Self, String> {
        let Some(value) = value else {
            return Ok(Self::disabled());
        };
        let mut allowed = HashSet::new();
        for token in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if token == "*" || token.eq_ignore_ascii_case("all") {
                return Ok(Self::only(Kernel::ALL));
            }
            allowed.insert(Kernel::parse(token)?);
        }
        Ok(Self {
            allowed: Some(allowed),
        })
    }

    pub fn is_active(&self) -> bool {
        self.allowed.is_some()
    }

    pub fn allows(&self, kernel: Kernel) -> bool {
        match &self.allowed {
            None => true,
            Some(allowed) => allowed.contains(&kernel),
        }
    }

    pub fn check(&self, kernel: Kernel) -> TargetDecision {
        if self.allows(kernel) {
            TargetDecision::Allowed
        } else {
            TargetDecision::Denied {
                kernel,
                allowed: self.describe(),
            }
        }
    }

    pub fn describe(&self) -> String {
        let Some(allowed) = &self.allowed else {
            return "off".into();
        };
        if allowed.is_empty() {
            return "<empty deny-all>".into();
        }
        let mut names: Vec<_> = Kernel::ALL
            .into_iter()
            .filter(|k| allowed.contains(k))
            .map(|k| k.program())
            .collect();
        names.sort_unstable();
        names.join(",")
    }
}

impl Default for TargetAllowlist {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetDecision {
    Allowed,
    Denied { kernel: Kernel, allowed: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_policy_is_inert_and_allows_every_kernel() {
        let p = TargetAllowlist::from_env(None).unwrap();
        assert!(!p.is_active());
        for k in Kernel::ALL {
            assert!(p.allows(k));
        }
    }

    #[test]
    fn empty_config_is_active_fail_closed_deny_all() {
        let p = TargetAllowlist::from_env(Some(" ")).unwrap();
        assert!(p.is_active());
        assert_eq!(p.describe(), "<empty deny-all>");
        for k in Kernel::ALL {
            assert!(!p.allows(k));
        }
    }

    #[test]
    fn named_config_allows_only_listed_kernels_with_aliases() {
        let p = TargetAllowlist::from_env(Some("loop_lib, hf")).unwrap();
        assert!(p.allows(Kernel::LoopLib));
        assert!(p.allows(Kernel::Handoff));
        assert!(!p.allows(Kernel::Atc));
        assert!(!p.allows(Kernel::Weave));
    }

    #[test]
    fn all_alias_allows_every_kernel() {
        let p = TargetAllowlist::from_env(Some("all")).unwrap();
        for k in Kernel::ALL {
            assert!(p.allows(k));
        }
    }

    #[test]
    fn unknown_kernel_name_is_a_config_error() {
        assert!(TargetAllowlist::from_env(Some("loop,nope")).is_err());
    }
}
