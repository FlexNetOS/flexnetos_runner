//! Idle / liveness watchdog policy.
//!
//! Wall-clock deadlines bound total runtime; liveness bounds **silence**. A kernel can be inside its
//! wall-clock deadline but deadlocked/no-output. This policy carries the optional idle timeout seam;
//! the subprocess invoker owns enforcement because it owns the child pipes.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LivenessPolicy {
    default_secs: Option<u64>,
}

impl LivenessPolicy {
    pub fn disabled() -> Self {
        Self { default_secs: None }
    }

    pub fn from_secs(secs: u64) -> Self {
        Self {
            default_secs: (secs > 0).then_some(secs),
        }
    }

    pub fn default_secs(&self) -> Option<u64> {
        self.default_secs
    }

    /// Effective idle timeout: the tighter of operator default and per-request value. A request can
    /// only shorten the operator cap, not lengthen it.
    pub fn effective(&self, requested_secs: Option<u64>) -> Option<Duration> {
        match (self.default_secs, requested_secs.filter(|s| *s > 0)) {
            (None, None) => None,
            (Some(a), None) => Some(Duration::from_secs(a)),
            (None, Some(b)) => Some(Duration::from_secs(b)),
            (Some(a), Some(b)) => Some(Duration::from_secs(a.min(b))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_without_request_is_inert() {
        assert_eq!(LivenessPolicy::disabled().effective(None), None);
    }

    #[test]
    fn request_can_set_or_shorten_timeout() {
        assert_eq!(
            LivenessPolicy::disabled().effective(Some(7)),
            Some(Duration::from_secs(7))
        );
        assert_eq!(
            LivenessPolicy::from_secs(10).effective(Some(3)),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            LivenessPolicy::from_secs(10).effective(Some(30)),
            Some(Duration::from_secs(10))
        );
    }
}
