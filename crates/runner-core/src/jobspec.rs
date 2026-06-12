//! The signed job spec the App dispatches to the runner over UDS (ADR-0008 S7).
//!
//! HMAC-SHA256 signed with a key sealed in envctl's vault; the runner verifies (constant
//! time) before routing. Serde-serializable so it rides the UDS frame as JSON.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// What the job asks the runner to do. Each variant maps to exactly one kernel (see
/// [`crate::router`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum JobKind {
    /// Build/test a ref → loop_lib fan-out.
    Ci { repo: String, head_sha: String },
    /// Run the merge-gate review for a PR → atc reviewer.
    ReviewGate {
        repo: String,
        pr_number: u64,
        head_sha: String,
    },
    /// A generic agent task → atc.
    AgentTask { repo: String, prompt_ref: String },
    /// A loop cycle (ship) → handoff `hf`.
    LoopCycle { repo: String, task_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobSpec {
    /// Unique job id (the runner's dedup key).
    pub id: String,
    /// Ties the job back to the originating webhook delivery / front-door request.
    pub correlation_id: String,
    /// Whether the triggering event came from a fork (drives isolation — see [`crate::safety`]).
    pub from_fork: bool,
    pub job: JobKind,
}

impl JobSpec {
    /// Canonical bytes signed/verified (stable struct-ordered JSON).
    pub fn signing_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("JobSpec serializes")
    }

    /// Produce the `sha256=<hex>` signature for this spec under `key`.
    pub fn sign(&self, key: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(&self.signing_bytes());
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    /// Verify a `sha256=<hex>` signature against this spec (constant-time).
    pub fn verify(&self, key: &[u8], signature: &str) -> Result<(), SignatureError> {
        let hex_sig = signature
            .trim()
            .strip_prefix("sha256=")
            .ok_or(SignatureError::Malformed)?;
        let sig = hex::decode(hex_sig).map_err(|_| SignatureError::Malformed)?;
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(&self.signing_bytes());
        mac.verify_slice(&sig).map_err(|_| SignatureError::Mismatch)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("malformed signature (expected `sha256=<hex>`)")]
    Malformed,
    #[error("signature mismatch")]
    Mismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> JobSpec {
        JobSpec {
            id: "job-1".into(),
            correlation_id: "delivery-9".into(),
            from_fork: false,
            job: JobKind::Ci {
                repo: "FlexNetOS/x".into(),
                head_sha: "abc".into(),
            },
        }
    }

    #[test]
    fn sign_verify_roundtrip() {
        let s = spec();
        let sig = s.sign(b"key");
        assert!(s.verify(b"key", &sig).is_ok());
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let s = spec();
        let sig = s.sign(b"key");
        assert_eq!(s.verify(b"other", &sig), Err(SignatureError::Mismatch));
    }

    #[test]
    fn verify_rejects_tampered_spec() {
        let s = spec();
        let sig = s.sign(b"key");
        let mut tampered = s.clone();
        tampered.id = "job-2".into();
        assert_eq!(tampered.verify(b"key", &sig), Err(SignatureError::Mismatch));
    }

    #[test]
    fn verify_rejects_malformed() {
        let s = spec();
        assert_eq!(s.verify(b"key", "md5=abc"), Err(SignatureError::Malformed));
    }

    #[test]
    fn json_roundtrip() {
        let s = spec();
        let json = serde_json::to_string(&s).unwrap();
        let back: JobSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
