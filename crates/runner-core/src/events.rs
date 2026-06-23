//! Dispatch event log — a structured audit trail of every admission decision (adapted from
//! kclaw0 `event-system.js`).
//!
//! kclaw0 emits structured NDJSON events for *observable agent behavior* (a fixed vocabulary:
//! `tool_call`, `loop_detected`, `checkpoint`, …). The runner-plane analogue is an **append-only,
//! one-JSON-object-per-line audit record of every dispatch decision** — verified/rejected/looped/
//! over-budget/delegated — keyed by the job fingerprint + correlation id. This is the audit/lineage
//! requirement for unattended autonomy (`meta/DARK-FACTORY-RESEARCH.md` §7, Goal G: "every
//! automated action has a witnessed record").
//!
//! `runner-core` stays I/O-free: it defines the event shape, NDJSON serialization, the [`EventSink`]
//! seam, and a no-op [`NullSink`]. The actual file append lives in the `runner-dispatch` binary
//! (a `FileSink` over `FXRUN_EVENT_LOG`), so the decision core remains pure and unit-testable with
//! a recording sink. The audit log is observability, orthogonal to model routing (weave's domain).

use crate::jobspec::JobSpec;
use crate::loopguard::fingerprint;
use serde::Serialize;

/// The outcome of one dispatch admission decision — the runner-plane event vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Frame bytes did not parse as a [`crate::wire::DispatchRequest`].
    Unparseable,
    /// HMAC verification failed (bad key / tampered body).
    VerifyFailed,
    /// Fork-triggered job refused (must run on hosted infra).
    ForkRejected,
    /// Runaway-loop circuit breaker tripped.
    LoopTripped,
    /// Dispatch budget exhausted (bounded-autonomy kill-switch).
    BudgetDenied,
    /// Routed and delegated to a kernel.
    Delegated,
    /// Delegation to the kernel failed.
    KernelFailed,
}

impl Outcome {
    /// Whether this outcome is a rejection (anything but a clean delegation).
    pub fn is_rejection(&self) -> bool {
        !matches!(self, Outcome::Delegated)
    }
}

/// One audit record. Serializes to a single JSON object (one NDJSON line). Job-identifying fields
/// are absent when the frame never parsed far enough to know them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DispatchEvent {
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Semantic fingerprint (same keying as the loop breaker) — ties an event to the *work*.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    /// Human-readable reason (e.g. the rejection message).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl DispatchEvent {
    /// An event for a decision made *before* the job parsed (no job fields).
    pub fn untied(outcome: Outcome, detail: impl Into<String>) -> Self {
        Self {
            outcome,
            job_id: None,
            correlation_id: None,
            fingerprint: None,
            kernel: None,
            detail: Some(detail.into()),
        }
    }

    /// An event tied to a verified job (fills id / correlation id / fingerprint).
    pub fn for_job(outcome: Outcome, job: &JobSpec) -> Self {
        Self {
            outcome,
            job_id: Some(job.id.clone()),
            correlation_id: Some(job.correlation_id.clone()),
            fingerprint: Some(fingerprint(job)),
            kernel: None,
            detail: None,
        }
    }

    /// Builder: attach the delegated kernel.
    pub fn with_kernel(mut self, kernel: impl Into<String>) -> Self {
        self.kernel = Some(kernel.into());
        self
    }

    /// Builder: attach a reason / detail string.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Render as a single NDJSON line (no trailing newline; the sink adds it).
    pub fn to_ndjson(&self) -> String {
        serde_json::to_string(self).expect("DispatchEvent serializes")
    }
}

/// The audit seam: receive each [`DispatchEvent`]. Implemented by a file appender in the binary and
/// a no-op / recorder in tests. `&self` (not `&mut`) so the dispatcher can hold it immutably; impls
/// use interior mutability or append-only I/O.
pub trait EventSink {
    fn emit(&self, event: &DispatchEvent);
}

/// Drops every event — the behaviour-preserving default when no audit log is configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: &DispatchEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobspec::JobKind;
    use std::cell::RefCell;

    fn job() -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "delivery-9".into(),
            from_fork: false,
            job: JobKind::Ci {
                repo: "FlexNetOS/meta".into(),
                head_sha: "abc".into(),
            },
        }
    }

    #[test]
    fn for_job_ties_event_to_the_work() {
        let e = DispatchEvent::for_job(Outcome::Delegated, &job()).with_kernel("loop");
        assert_eq!(e.job_id.as_deref(), Some("job-1"));
        assert_eq!(e.correlation_id.as_deref(), Some("delivery-9"));
        assert_eq!(e.fingerprint, Some(fingerprint(&job())));
        assert_eq!(e.kernel.as_deref(), Some("loop"));
        assert!(!e.outcome.is_rejection());
    }

    #[test]
    fn ndjson_is_single_line_and_skips_absent_fields() {
        let line = DispatchEvent::untied(Outcome::Unparseable, "bad frame").to_ndjson();
        assert!(!line.contains('\n'));
        assert!(line.contains(r#""outcome":"unparseable""#));
        assert!(line.contains(r#""detail":"bad frame""#));
        // Absent job fields are omitted, not null.
        assert!(!line.contains("job_id"));
        assert!(!line.contains("null"));
    }

    #[test]
    fn rejection_classification() {
        assert!(Outcome::ForkRejected.is_rejection());
        assert!(Outcome::LoopTripped.is_rejection());
        assert!(Outcome::BudgetDenied.is_rejection());
        assert!(!Outcome::Delegated.is_rejection());
    }

    #[test]
    fn null_sink_is_a_noop() {
        NullSink.emit(&DispatchEvent::for_job(Outcome::Delegated, &job()));
    }

    #[test]
    fn sink_trait_records_through_interior_mutability() {
        // Proves the &self sink contract supports a recorder (as the binary's FileSink does via I/O).
        struct Recorder(RefCell<Vec<Outcome>>);
        impl EventSink for Recorder {
            fn emit(&self, e: &DispatchEvent) {
                self.0.borrow_mut().push(e.outcome);
            }
        }
        let r = Recorder(RefCell::new(Vec::new()));
        r.emit(&DispatchEvent::untied(Outcome::VerifyFailed, "x"));
        r.emit(&DispatchEvent::for_job(Outcome::Delegated, &job()));
        assert_eq!(
            r.0.into_inner(),
            vec![Outcome::VerifyFailed, Outcome::Delegated]
        );
    }
}
