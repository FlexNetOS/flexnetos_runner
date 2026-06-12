//! JIT / ephemeral runner lifecycle (ADR-0008 §2): register a just-in-time runner that
//! runs exactly one job, then auto-removes — no long-lived registration token, no
//! cross-job persistence. P0 models the request body + the state machine; the live
//! `generate-jitconfig` call and agent supervision land in P1.

use serde::Serialize;

/// Body for `POST /orgs/{org}/actions/runners/generate-jitconfig` (returns `encoded_jit_config`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JitConfigRequest {
    pub name: String,
    pub runner_group_id: u64,
    pub labels: Vec<String>,
    pub work_folder: String,
}

impl JitConfigRequest {
    pub fn new(name: impl Into<String>, runner_group_id: u64, labels: Vec<String>) -> Self {
        Self {
            name: name.into(),
            runner_group_id,
            labels,
            work_folder: "_work".into(),
        }
    }
}

/// Lifecycle states of an ephemeral runner (strictly linear: one job, then removed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Unregistered,
    Registered,
    Running,
    Completed,
    Removed,
}

impl State {
    /// The only legal forward transition (ephemeral: no `Running → Running` reuse).
    pub fn next(self) -> Option<State> {
        match self {
            State::Unregistered => Some(State::Registered),
            State::Registered => Some(State::Running),
            State::Running => Some(State::Completed),
            State::Completed => Some(State::Removed),
            State::Removed => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_request_defaults_work_folder() {
        let r = JitConfigRequest::new("rnr-1", 3, vec!["self-hosted".into(), "flexnetos".into()]);
        assert_eq!(r.work_folder, "_work");
        assert_eq!(r.name, "rnr-1");
        assert_eq!(r.runner_group_id, 3);
    }

    #[test]
    fn lifecycle_is_linear_and_terminates() {
        let mut s = State::Unregistered;
        let mut steps = 0;
        while let Some(n) = s.next() {
            s = n;
            steps += 1;
            assert!(steps < 10, "lifecycle must terminate");
        }
        assert_eq!(s, State::Removed);
        assert_eq!(steps, 4);
    }
}
