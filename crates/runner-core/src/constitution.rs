//! Constitution-immutability gate (adapted from `Conway-Research/automaton`'s protected,
//! git-versioned **constitution files that cannot be modified**, and kclaw0
//! `dark-factory.js::verifyImmutability`, which SHA-256s `MISSION.md`/`FACTORY_RULES.md`/`CLAUDE.md`
//! and refuses to act if any changed).
//!
//! The runner-plane analogue: the dispatcher **seals** the SHA-256 of its own governing files
//! (e.g. `.handoff/policy.toml`, the fork-PR denylist, `CLAUDE.md`) at startup, then re-checks them
//! before each dispatch. If any sealed file changes or disappears while the autonomous server is
//! running, dispatch is refused — an agent in the loop cannot **weaken its own guardrails** mid-run.
//! This is the documented self-evolving-agent failure mode (the Darwin-Gödel Machine *deleted the
//! reward markers meant to catch it cheating* — `meta/DARK-FACTORY-RESEARCH.md` §5).
//!
//! It is the first admission gate, matching dark-factory's `immutability → budget → … ` order:
//! a tampered constitution refuses *everything*, before parsing the frame.
//!
//! `runner-core` stays I/O-free: hashing is over bytes the caller supplies via a `read` closure
//! (the binary passes `std::fs::read`, tests pass a fake). Empty seal (no files configured) is
//! always [`ConstitutionStatus::Intact`] — behaviour-preserving.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// SHA-256 hex of `bytes`.
pub fn hash(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// The sealed fingerprints of the runner's governing files: `name → sha256`. Captured once at
/// startup; immutable thereafter (the baseline every later check compares against).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Constitution {
    sealed: BTreeMap<String, String>,
}

impl Constitution {
    /// Seal the named files by hashing their current contents (via `read`). A name that can't be
    /// read at seal time is skipped (it isn't part of the constitution) — only files that exist now
    /// are protected against later change.
    pub fn seal<R>(names: &[String], read: R) -> Self
    where
        R: Fn(&str) -> Option<Vec<u8>>,
    {
        let sealed = names
            .iter()
            .filter_map(|n| read(n).map(|bytes| (n.clone(), hash(&bytes))))
            .collect();
        Self { sealed }
    }

    /// Number of sealed files.
    pub fn len(&self) -> usize {
        self.sealed.len()
    }

    /// Whether nothing is sealed (the gate is inert — the behaviour-preserving default).
    pub fn is_empty(&self) -> bool {
        self.sealed.is_empty()
    }

    /// The sealed file names (sorted).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sealed.keys().map(String::as_str)
    }

    /// Re-check the sealed files against their current contents (via `read`). A sealed file whose
    /// content now hashes differently, or which can no longer be read (deleted), is a violation.
    /// Files outside the seal are ignored. An empty constitution is always intact.
    pub fn verify<R>(&self, read: R) -> ConstitutionStatus
    where
        R: Fn(&str) -> Option<Vec<u8>>,
    {
        let mut changed = Vec::new();
        for (name, sealed_hash) in &self.sealed {
            match read(name) {
                Some(bytes) if &hash(&bytes) == sealed_hash => {}
                _ => changed.push(name.clone()),
            }
        }
        if changed.is_empty() {
            ConstitutionStatus::Intact
        } else {
            ConstitutionStatus::Violated { changed }
        }
    }
}

/// The result of re-checking the constitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstitutionStatus {
    /// Every sealed file is unchanged (or nothing is sealed).
    Intact,
    /// One or more sealed files changed or vanished — the runner's guardrails were tampered with.
    Violated { changed: Vec<String> },
}

impl ConstitutionStatus {
    /// Whether the constitution was tampered with (dispatch must be refused).
    pub fn is_violated(&self) -> bool {
        matches!(self, ConstitutionStatus::Violated { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn store(pairs: &[(&str, &str)]) -> HashMap<String, Vec<u8>> {
        pairs
            .iter()
            .map(|(n, c)| (n.to_string(), c.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn intact_when_files_unchanged() {
        let files = store(&[
            ("policy.toml", "require_review = true"),
            ("CLAUDE.md", "rules"),
        ]);
        let names = vec!["policy.toml".to_string(), "CLAUDE.md".to_string()];
        let c = Constitution::seal(&names, |n| files.get(n).cloned());
        assert_eq!(c.len(), 2);
        assert_eq!(
            c.verify(|n| files.get(n).cloned()),
            ConstitutionStatus::Intact
        );
    }

    #[test]
    fn violated_when_a_sealed_file_changes() {
        let names = vec!["policy.toml".to_string()];
        let orig = store(&[("policy.toml", "require_review = true")]);
        let c = Constitution::seal(&names, |n| orig.get(n).cloned());
        // Tamper: weaken the guardrail.
        let tampered = store(&[("policy.toml", "require_review = false")]);
        let status = c.verify(|n| tampered.get(n).cloned());
        assert!(status.is_violated());
        assert_eq!(
            status,
            ConstitutionStatus::Violated {
                changed: vec!["policy.toml".to_string()]
            }
        );
    }

    #[test]
    fn violated_when_a_sealed_file_is_deleted() {
        let names = vec!["policy.toml".to_string()];
        let orig = store(&[("policy.toml", "x")]);
        let c = Constitution::seal(&names, |n| orig.get(n).cloned());
        // The file vanishes.
        let status = c.verify(|_| None);
        assert!(status.is_violated());
    }

    #[test]
    fn empty_constitution_is_always_intact() {
        let c = Constitution::seal(&[], |_| None);
        assert!(c.is_empty());
        assert_eq!(c.verify(|_| None), ConstitutionStatus::Intact);
    }

    #[test]
    fn missing_file_at_seal_time_is_not_protected() {
        // Only files that exist when sealed become part of the constitution.
        let files = store(&[("exists.toml", "a")]);
        let names = vec!["exists.toml".to_string(), "absent.toml".to_string()];
        let c = Constitution::seal(&names, |n| files.get(n).cloned());
        assert_eq!(c.len(), 1);
        assert_eq!(c.names().collect::<Vec<_>>(), vec!["exists.toml"]);
    }

    #[test]
    fn extra_files_outside_the_seal_are_ignored() {
        let names = vec!["a".to_string()];
        let orig = store(&[("a", "1")]);
        let c = Constitution::seal(&names, |n| orig.get(n).cloned());
        // A new unrelated file appears; the sealed one is unchanged → still intact.
        let now = store(&[("a", "1"), ("b", "2")]);
        assert_eq!(
            c.verify(|n| now.get(n).cloned()),
            ConstitutionStatus::Intact
        );
    }
}
