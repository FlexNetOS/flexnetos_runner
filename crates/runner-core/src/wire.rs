//! The signed UDS dispatch frame (ADR-0008 §2/S7): the envelope the App sends the dispatcher,
//! and the dispatcher's reply.
//!
//! ## Robustness rule — *sign what you send*
//! The frame carries the [`JobSpec`] as the exact JSON **string** the client signed
//! (`spec_json`), and the signature is HMAC-SHA256 over those exact bytes. The server verifies
//! over the bytes it *received*, then parses — so the App and the runner never need
//! byte-identical re-serialization (the same discipline as GitHub webhook body verification).
//! A nested re-serialize on the far side would be brittle; this is not.
//!
//! The HMAC key is sealed in envctl's vault and injected at runtime (P3); the App holds the same
//! key to sign. Verification is constant-time (`Mac::verify_slice`).

use crate::jobspec::JobSpec;
use crate::recovery::RecoveryDirective;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// The request envelope sent over the UDS socket (one JSON object per connection).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatchRequest {
    /// The exact JSON text of the [`JobSpec`] that was signed.
    pub spec_json: String,
    /// `sha256=<hex>` HMAC over `spec_json`'s bytes.
    pub signature: String,
    /// Optional human-approval grant (present only on a re-dispatch *after* a human approved a job
    /// whose class requires it — see [`crate::approval`]). Absent on a normal first dispatch, so
    /// older App frames stay valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
    /// Optional per-job wall-clock deadline in seconds (see [`crate::deadline`]). An out-of-band
    /// request the App attaches to the envelope; the runner takes the **tighter** of this and its
    /// operator ceiling, so it can only shorten a job, never exceed the cap. Absent on older frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_secs: Option<u64>,
}

/// A human-approval grant: HMAC proof, over the job's fingerprint, that an approver authorized this
/// specific job. Forging it requires the dispatch key (the App holds it and computes the grant once
/// a human approves), and binding to the fingerprint stops a grant being replayed onto a *different*
/// job. Carried on the [`DispatchRequest`] envelope, not the signed `JobSpec`, so approval is an
/// out-of-band fact the orchestrator attaches without re-minting the original signed spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Approval {
    /// Approver identity (informational, and bound into the signature so it can't be swapped).
    pub by: String,
    /// `sha256=<hex>` HMAC over `approve:<fingerprint>:<by>`.
    pub signature: String,
}

impl Approval {
    /// The bytes an approval signs: `approve:<fingerprint>:<by>`.
    fn grant_message(fingerprint: &str, by: &str) -> Vec<u8> {
        format!("approve:{fingerprint}:{by}").into_bytes()
    }

    /// Mint a grant for `fingerprint` by approver `by`, signed with the dispatch `key` (the App
    /// calls this after a human approves).
    pub fn grant(key: &[u8], fingerprint: &str, by: impl Into<String>) -> Self {
        let by = by.into();
        let signature = sign_bytes(key, &Self::grant_message(fingerprint, &by));
        Self { by, signature }
    }

    /// Verify this grant authorizes `fingerprint` under `key` (constant-time).
    pub fn verify(&self, key: &[u8], fingerprint: &str) -> bool {
        verify_bytes(
            key,
            &Self::grant_message(fingerprint, &self.by),
            &self.signature,
        )
        .is_ok()
    }
}

/// The dispatcher's reply. `accepted=false` always carries an `error`; the optional fields echo
/// the routing decision for an accepted job (so the caller can observe the delegation).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatchResponse {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// For a rejection, the runner's advice on what to do next — retry-with-backoff or escalate to a
    /// human (the [`RecoveryDirective`]). The orchestrator (App / weave) owns the timer and the
    /// escalation PR; the runner only recommends. Absent on acceptance and on pre-parse rejections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryDirective>,
}

impl DispatchResponse {
    /// A rejection carrying the reason (fail-closed default for every non-happy path).
    pub fn rejected(error: impl Into<String>) -> Self {
        Self {
            accepted: false,
            kernel: None,
            placement: None,
            intent: None,
            error: Some(error.into()),
            recovery: None,
        }
    }

    /// Attach a recovery directive to a rejection (builder).
    pub fn with_recovery(mut self, recovery: RecoveryDirective) -> Self {
        self.recovery = Some(recovery);
        self
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error("malformed dispatch frame: {0}")]
    Malformed(String),
    #[error("signature mismatch")]
    Signature,
}

/// Client side: serialize + sign a [`JobSpec`] into a [`DispatchRequest`].
pub fn sign_frame(key: &[u8], spec: &JobSpec) -> Result<DispatchRequest, WireError> {
    let spec_json = serde_json::to_string(spec).map_err(|e| WireError::Malformed(e.to_string()))?;
    let signature = sign_bytes(key, spec_json.as_bytes());
    Ok(DispatchRequest {
        spec_json,
        signature,
        approval: None,
        deadline_secs: None,
    })
}

/// Server side: verify the frame signature over the exact received bytes, THEN parse the spec.
/// Order matters — an unverified body is never parsed.
pub fn verify_frame(key: &[u8], req: &DispatchRequest) -> Result<JobSpec, WireError> {
    verify_bytes(key, req.spec_json.as_bytes(), &req.signature)?;
    serde_json::from_str(&req.spec_json).map_err(|e| WireError::Malformed(e.to_string()))
}

fn sign_bytes(key: &[u8], msg: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn verify_bytes(key: &[u8], msg: &[u8], signature: &str) -> Result<(), WireError> {
    let hex_sig = signature
        .trim()
        .strip_prefix("sha256=")
        .ok_or_else(|| WireError::Malformed("expected `sha256=<hex>`".into()))?;
    let sig = hex::decode(hex_sig).map_err(|_| WireError::Malformed("signature not hex".into()))?;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.verify_slice(&sig).map_err(|_| WireError::Signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobspec::{JobKind, JobSpec};

    fn spec() -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "delivery-9".into(),
            from_fork: false,
            job: JobKind::Ci {
                repo: "FlexNetOS/meta".into(),
                head_sha: "abc123".into(),
            },
        }
    }

    #[test]
    fn sign_then_verify_roundtrips() {
        let frame = sign_frame(b"k", &spec()).unwrap();
        let got = verify_frame(b"k", &frame).unwrap();
        assert_eq!(got, spec());
    }

    #[test]
    fn wrong_key_is_signature_error() {
        let frame = sign_frame(b"k", &spec()).unwrap();
        assert_eq!(verify_frame(b"other", &frame), Err(WireError::Signature));
    }

    #[test]
    fn tampered_body_is_signature_error() {
        let mut frame = sign_frame(b"k", &spec()).unwrap();
        // Same signature, body swapped → the MAC no longer matches the bytes.
        frame.spec_json = frame.spec_json.replace("job-1", "job-2");
        assert_eq!(verify_frame(b"k", &frame), Err(WireError::Signature));
    }

    #[test]
    fn malformed_signature_is_malformed() {
        let mut frame = sign_frame(b"k", &spec()).unwrap();
        frame.signature = "md5=zzzz".into();
        assert_eq!(
            verify_frame(b"k", &frame),
            Err(WireError::Malformed("expected `sha256=<hex>`".into()))
        );
    }

    #[test]
    fn valid_signature_over_non_jobspec_is_malformed() {
        // A correctly-signed body that isn't a JobSpec passes the MAC but fails the parse.
        let body = "not a job spec";
        let frame = DispatchRequest {
            spec_json: body.to_string(),
            signature: sign_bytes(b"k", body.as_bytes()),
            approval: None,
            deadline_secs: None,
        };
        assert!(matches!(
            verify_frame(b"k", &frame),
            Err(WireError::Malformed(_))
        ));
    }

    #[test]
    fn response_rejected_has_error_and_skips_none_fields() {
        let r = DispatchResponse::rejected("nope");
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"accepted":false,"error":"nope"}"#);
    }

    #[test]
    fn approval_grant_round_trips_and_binds_to_the_fingerprint() {
        let grant = Approval::grant(b"k", "fp-123", "alice");
        assert!(grant.verify(b"k", "fp-123"));
        // Wrong key, wrong fingerprint, or swapped approver all fail.
        assert!(!grant.verify(b"other", "fp-123"));
        assert!(!grant.verify(b"k", "fp-999"));
        let mut tampered = grant.clone();
        tampered.by = "mallory".into();
        assert!(!tampered.verify(b"k", "fp-123"));
    }

    #[test]
    fn frame_without_approval_omits_the_field_and_still_parses() {
        let frame = sign_frame(b"k", &spec()).unwrap();
        assert!(frame.approval.is_none());
        let json = serde_json::to_string(&frame).unwrap();
        assert!(!json.contains("approval"));
        let back: DispatchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, frame);
    }

    #[test]
    fn deadline_secs_is_optional_omitted_when_absent_and_round_trips_when_present() {
        // Default frame: no deadline field on the wire (older App frames stay byte-clean).
        let frame = sign_frame(b"k", &spec()).unwrap();
        assert_eq!(frame.deadline_secs, None);
        assert!(!serde_json::to_string(&frame).unwrap().contains("deadline"));

        // A frame that requests a deadline carries it and round-trips.
        let mut with_deadline = sign_frame(b"k", &spec()).unwrap();
        with_deadline.deadline_secs = Some(45);
        let json = serde_json::to_string(&with_deadline).unwrap();
        assert!(json.contains(r#""deadline_secs":45"#));
        let back: DispatchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, with_deadline);

        // An older frame JSON without the field still parses (serde default → None).
        let legacy = r#"{"spec_json":"x","signature":"sha256=00"}"#;
        let parsed: DispatchRequest = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.deadline_secs, None);
    }
}
