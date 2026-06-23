//! Secret redaction for the audit / error egress surfaces (adapted from `coleam00/Archon`'s
//! `repo.ts`, which **scrubs the auth token out of every error string before it is classified,
//! logged, or returned** — so a failing git op can never echo the credential it was handed).
//!
//! The runner-plane analogue: the dispatcher holds key material (the HMAC dispatch key; in P3 the
//! envctl-injected bearer / approval-grant tokens) and writes two operator-readable surfaces that
//! could otherwise carry it verbatim — the NDJSON audit log ([`crate::events`] `detail` fields) and
//! the UDS error reply ([`crate::wire::DispatchResponse::error`]). A formatted error or recovery
//! string that happened to splice in a secret would leak it into a log file or back over the socket.
//! [`Redactor`] is the single choke point that replaces every occurrence of a known secret with
//! [`Redactor::PLACEHOLDER`] *before* the text reaches either surface.
//!
//! `runner-core` stays I/O-free: this module only *computes* the scrubbed string (and offers the
//! pure [`RedactingSink`] decorator over the [`EventSink`] seam). The binary builds the [`Redactor`]
//! from its configured secrets, wraps its file sink in a [`RedactingSink`], and scrubs the response
//! `error` on the way out. Orthogonal to model routing (weave's domain).
//!
//! **Behaviour-preserving:** a [`Redactor`] with no registered secrets (or one whose secrets never
//! appear) returns the input borrowed, unchanged — zero allocation, byte-identical output. It is
//! defense-in-depth: today's messages don't splice secrets, but the seam guarantees they never can.
//!
//! **Minimum length.** A secret shorter than [`Redactor::MIN_SECRET_LEN`] is *not* registered: a
//! 1–3 character "secret" would match incidental substrings and corrupt unrelated audit text, which
//! is worse than the leak it guards. Real key material (the envctl-sealed dispatch key is 32 bytes)
//! is always well above the floor; the floor only rejects pathological/test stand-ins.

use crate::events::{DispatchEvent, EventSink};
use std::borrow::Cow;

/// A set of secret strings that must never appear in operator-readable output, and the machinery to
/// scrub them. Cheap to clone (a `Vec<String>` built once at startup); `Clone` so the binary can keep
/// one copy for the response path and hand another to the [`RedactingSink`].
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    /// Registered secrets, longest first (so a secret that is a *substring* of a longer one never
    /// pre-empts the longer match).
    secrets: Vec<String>,
}

impl Redactor {
    /// What a redacted secret is replaced with. Visibly non-secret and unlikely to occur in real
    /// audit text, so its presence in a log is an unambiguous "a secret was scrubbed here" marker.
    pub const PLACEHOLDER: &'static str = "«redacted»";

    /// The shortest string accepted as a secret. Below this a "secret" would match incidental text
    /// and mangle unrelated output; real key material is far longer (see the module docs).
    pub const MIN_SECRET_LEN: usize = 4;

    /// An empty redactor (registers nothing) — the behaviour-preserving default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `secret` if it qualifies (long enough and not already present). Returns whether it
    /// was added. Keeps the set ordered longest-first.
    pub fn register(&mut self, secret: &str) -> bool {
        if secret.len() < Self::MIN_SECRET_LEN || self.secrets.iter().any(|s| s == secret) {
            return false;
        }
        self.secrets.push(secret.to_string());
        // Longest first: a substring secret must not consume a longer secret's match.
        self.secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        true
    }

    /// Builder form of [`Self::register`] (a non-qualifying secret is silently skipped).
    pub fn with_secret(mut self, secret: impl AsRef<str>) -> Self {
        self.register(secret.as_ref());
        self
    }

    /// Build a redactor from an iterator of candidate secrets (each filtered by [`Self::register`]).
    pub fn from_secrets<I, S>(secrets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut r = Self::new();
        for s in secrets {
            r.register(s.as_ref());
        }
        r
    }

    /// Whether any secret is registered (i.e. redaction will actually do something).
    pub fn is_active(&self) -> bool {
        !self.secrets.is_empty()
    }

    /// How many secrets are registered.
    pub fn secret_count(&self) -> usize {
        self.secrets.len()
    }

    /// Replace every occurrence of every registered secret in `text` with [`Self::PLACEHOLDER`].
    /// Returns the input **borrowed and unchanged** when no secret is present (the common path — no
    /// allocation, byte-identical), or an owned scrubbed string otherwise.
    pub fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut out: Cow<'a, str> = Cow::Borrowed(text);
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = Cow::Owned(out.replace(secret.as_str(), Self::PLACEHOLDER));
            }
        }
        out
    }
}

/// An [`EventSink`] decorator that scrubs each event's free-text `detail` through a [`Redactor`]
/// before forwarding to the inner sink — so a secret spliced into a formatted error/recovery string
/// can never reach the on-disk audit log. The structured identity fields (`job_id`,
/// `correlation_id`, `fingerprint`, `kernel`) are *not* touched: they carry lineage keys, not
/// free-text, and rewriting them would break audit correlation. Pure (no I/O); the inner sink owns
/// the actual write. When nothing in `detail` matches, the original event is forwarded **without a
/// clone** (the redactor returns it borrowed).
#[derive(Debug, Clone)]
pub struct RedactingSink<S> {
    inner: S,
    redactor: Redactor,
}

impl<S> RedactingSink<S> {
    /// Wrap `inner`, scrubbing every emitted event's `detail` through `redactor`.
    pub fn new(inner: S, redactor: Redactor) -> Self {
        Self { inner, redactor }
    }

    /// Unwrap, returning the inner sink.
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: EventSink> EventSink for RedactingSink<S> {
    fn emit(&self, event: &DispatchEvent) {
        match &event.detail {
            Some(detail) => match self.redactor.redact(detail) {
                // Nothing matched — forward the original event untouched (no allocation).
                Cow::Borrowed(_) => self.inner.emit(event),
                // A secret was scrubbed — forward a copy carrying the redacted detail.
                Cow::Owned(scrubbed) => {
                    let mut redacted = event.clone();
                    redacted.detail = Some(scrubbed);
                    self.inner.emit(&redacted);
                }
            },
            None => self.inner.emit(event),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{DispatchEvent, Outcome};
    use std::cell::RefCell;

    #[test]
    fn empty_redactor_is_inactive_and_borrows_unchanged() {
        let r = Redactor::new();
        assert!(!r.is_active());
        assert_eq!(r.secret_count(), 0);
        // No secrets → input returned borrowed (same allocation), byte-identical.
        assert!(matches!(
            r.redact("anything at all"),
            Cow::Borrowed("anything at all")
        ));
    }

    #[test]
    fn redacts_every_occurrence_of_a_secret() {
        let r = Redactor::new().with_secret("hunter2pass");
        let out = r.redact("login hunter2pass then retry hunter2pass");
        assert_eq!(out, "login «redacted» then retry «redacted»");
        assert!(!out.contains("hunter2pass"));
    }

    #[test]
    fn absent_secret_returns_borrowed() {
        let r = Redactor::new().with_secret("topsecretkey");
        // Active redactor, but the secret isn't present → still borrowed (no needless allocation).
        assert!(matches!(
            r.redact("clean text"),
            Cow::Borrowed("clean text")
        ));
    }

    #[test]
    fn too_short_secrets_are_not_registered() {
        // Below MIN_SECRET_LEN: refused, so it can't mangle incidental text.
        let mut r = Redactor::new();
        assert!(!r.register("abc")); // 3 chars
        assert!(!r.is_active());
        assert_eq!(r.redact("abc abc abc"), "abc abc abc");
        // At the floor it registers.
        assert!(r.register("abcd"));
        assert_eq!(r.redact("abcd!"), "«redacted»!");
    }

    #[test]
    fn duplicate_secrets_are_ignored() {
        let mut r = Redactor::new();
        assert!(r.register("repeated-secret"));
        assert!(!r.register("repeated-secret"));
        assert_eq!(r.secret_count(), 1);
    }

    #[test]
    fn longest_secret_wins_when_one_contains_another() {
        // "abcd" is a substring of "abcdef"; longest-first ordering must scrub the full token, not
        // leave a dangling "ef" behind.
        let r = Redactor::from_secrets(["abcd", "abcdef"]);
        assert_eq!(r.redact("value=abcdef end"), "value=«redacted» end");
    }

    #[test]
    fn from_secrets_filters_and_dedups() {
        let r = Redactor::from_secrets(["", "no", "goodsecret", "goodsecret", "alsofine"]);
        // "" and "no" are below the floor; "goodsecret" dedups → 2 registered.
        assert_eq!(r.secret_count(), 2);
    }

    /// A recording sink that captures the `detail` it actually received (post-decorator).
    struct DetailRecorder(RefCell<Vec<Option<String>>>);
    impl EventSink for DetailRecorder {
        fn emit(&self, e: &DispatchEvent) {
            self.0.borrow_mut().push(e.detail.clone());
        }
    }

    #[test]
    fn redacting_sink_scrubs_detail_before_the_inner_sink_sees_it() {
        let inner = DetailRecorder(RefCell::new(Vec::new()));
        let sink = RedactingSink::new(inner, Redactor::new().with_secret("leaked-key-material"));
        sink.emit(&DispatchEvent::untied(
            Outcome::KernelFailed,
            "kernel error: connecting with leaked-key-material failed",
        ));
        // A detail with no secret passes through verbatim.
        sink.emit(&DispatchEvent::untied(
            Outcome::VerifyFailed,
            "frame rejected: signature mismatch",
        ));
        // An event with no detail at all is forwarded untouched.
        sink.emit(&DispatchEvent::untied(Outcome::Delegated, "")); // empty detail, no match
        let seen = sink.into_inner().0.into_inner();
        assert_eq!(
            seen[0].as_deref(),
            Some("kernel error: connecting with «redacted» failed")
        );
        assert!(!seen[0].as_deref().unwrap().contains("leaked-key-material"));
        assert_eq!(
            seen[1].as_deref(),
            Some("frame rejected: signature mismatch")
        );
        assert_eq!(seen[2].as_deref(), Some(""));
    }
}
