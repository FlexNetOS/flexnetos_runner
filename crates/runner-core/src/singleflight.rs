//! Per-target single-flight admission mutex.
//!
//! This is the buildable half of the cycle-16 concurrency backlog. The dispatcher is still
//! one-connection-at-a-time, so a global max-in-flight cap would be inert until a concurrent serve
//! loop exists. The **per-mutable-target** lock is useful as a seam now: it gives the runner a typed,
//! deterministic older-wins admission rule for the day multiple dispatches can overlap, and it can be
//! unit-tested without threads.

use crate::jobspec::{JobKind, JobSpec};
use serde::Serialize;
use std::collections::HashMap;

/// A stable mutable-target key. Today this is the repository path for every job class: all runner
/// work mutates or verifies state in a repo/worktree, so concurrent jobs for the same repo are the
/// collision we must serialize. Kept as a newtype so future worktree/subpath targets can extend it
/// without changing the ledger API.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TargetKey(String);

impl TargetKey {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into().trim().to_ascii_lowercase())
    }

    pub fn for_job(job: &JobSpec) -> Self {
        match &job.job {
            JobKind::Ci { repo, .. }
            | JobKind::ReviewGate { repo, .. }
            | JobKind::AgentTask { repo, .. }
            | JobKind::LoopCycle { repo, .. } => Self::new(repo),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TargetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveFlight {
    job_id: String,
    sequence: u64,
}

/// A successfully acquired single-flight lease. The dispatcher releases it after the dispatch reaches
/// a terminal decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlightLease {
    target: TargetKey,
    job_id: String,
    sequence: u64,
}

impl FlightLease {
    pub fn target(&self) -> &TargetKey {
        &self.target
    }

    pub fn job_id(&self) -> &str {
        &self.job_id
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

/// Denial when another in-flight job already owns a target. The incumbent is always older: the
/// ledger assigns monotonically increasing sequence numbers, so the deterministic tiebreak is
/// "older wins, newer waits/escalates".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleFlightDenied {
    pub target: TargetKey,
    pub holder_job_id: String,
    pub holder_sequence: u64,
    pub incoming_job_id: String,
    pub incoming_sequence: u64,
}

/// Stateful active-target ledger held by the dispatcher across connections.
#[derive(Debug, Default, Clone)]
pub struct SingleFlight {
    active: HashMap<TargetKey, ActiveFlight>,
    next_sequence: u64,
}

impl SingleFlight {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    /// Try to acquire the job's mutable target. On conflict, returns a denial whose incumbent is the
    /// older in-flight job.
    pub fn try_acquire(&mut self, job: &JobSpec) -> Result<FlightLease, SingleFlightDenied> {
        let target = TargetKey::for_job(job);
        let incoming_sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        if let Some(holder) = self.active.get(&target) {
            return Err(SingleFlightDenied {
                target,
                holder_job_id: holder.job_id.clone(),
                holder_sequence: holder.sequence,
                incoming_job_id: job.id.clone(),
                incoming_sequence,
            });
        }
        let lease = FlightLease {
            target: target.clone(),
            job_id: job.id.clone(),
            sequence: incoming_sequence,
        };
        self.active.insert(
            target,
            ActiveFlight {
                job_id: job.id.clone(),
                sequence: incoming_sequence,
            },
        );
        Ok(lease)
    }

    /// Release a lease. Returns true only when it matched the active holder; stale/duplicate releases
    /// are ignored and return false.
    pub fn release(&mut self, lease: &FlightLease) -> bool {
        let Some(active) = self.active.get(lease.target()) else {
            return false;
        };
        if active.job_id == lease.job_id && active.sequence == lease.sequence {
            self.active.remove(lease.target());
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, repo: &str) -> JobSpec {
        JobSpec {
            id: id.into(),
            correlation_id: format!("c-{id}"),
            from_fork: false,
            job: JobKind::Ci {
                repo: repo.into(),
                head_sha: "abc".into(),
            },
        }
    }

    #[test]
    fn first_job_for_target_acquires_and_release_frees_it() {
        let mut sf = SingleFlight::new();
        let lease = sf.try_acquire(&job("j1", "FlexNetOS/Meta")).unwrap();
        assert_eq!(lease.target().as_str(), "flexnetos/meta");
        assert_eq!(sf.active_len(), 1);
        assert!(sf.release(&lease));
        assert_eq!(sf.active_len(), 0);
        assert!(sf.try_acquire(&job("j2", "flexnetos/meta")).is_ok());
    }

    #[test]
    fn same_target_conflict_denies_newer_job_with_older_wins_metadata() {
        let mut sf = SingleFlight::new();
        let holder = sf.try_acquire(&job("older", "FlexNetOS/meta")).unwrap();
        let denied = sf.try_acquire(&job("newer", "flexnetos/META")).unwrap_err();
        assert_eq!(denied.target.as_str(), "flexnetos/meta");
        assert_eq!(denied.holder_job_id, "older");
        assert_eq!(denied.holder_sequence, holder.sequence());
        assert_eq!(denied.incoming_job_id, "newer");
        assert!(denied.incoming_sequence > denied.holder_sequence);
    }

    #[test]
    fn different_targets_can_be_in_flight_together() {
        let mut sf = SingleFlight::new();
        assert!(sf.try_acquire(&job("a", "FlexNetOS/a")).is_ok());
        assert!(sf.try_acquire(&job("b", "FlexNetOS/b")).is_ok());
        assert_eq!(sf.active_len(), 2);
    }

    #[test]
    fn stale_lease_cannot_release_a_new_holder() {
        let mut sf = SingleFlight::new();
        let lease = sf.try_acquire(&job("a", "FlexNetOS/a")).unwrap();
        assert!(sf.release(&lease));
        let new_lease = sf.try_acquire(&job("b", "FlexNetOS/a")).unwrap();
        assert!(!sf.release(&lease));
        assert_eq!(sf.active_len(), 1);
        assert!(sf.release(&new_lease));
    }
}
