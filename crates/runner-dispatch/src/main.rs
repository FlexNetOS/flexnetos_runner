//! `fxrun-dispatch` — the meta-native dispatcher (ADR-0008 §2/S7).
//!
//! P0 reads a JSON `JobSpec` on stdin, verifies its signature (if a key is configured),
//! and prints the kernel plan + placement (dry-run). The UDS listener and the actual
//! kernel subprocess invocation (delegating to loop_lib/atc/handoff/weave — never
//! reimplementing them) land in P2; the dispatch key comes from envctl's vault in P3.

use runner_core::{jobspec::JobSpec, router, safety};
use std::io::Read;

fn main() -> anyhow::Result<()> {
    // P3: fetch from envctl's vault, not the environment.
    let key = std::env::var("FXRUN_DISPATCH_KEY").unwrap_or_default();
    let sig = std::env::var("FXRUN_DISPATCH_SIG").unwrap_or_default();

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        eprintln!(
            "fxrun-dispatch P0: pipe a JSON JobSpec on stdin; set FXRUN_DISPATCH_KEY + \
             FXRUN_DISPATCH_SIG (sha256=..) to verify it."
        );
        return Ok(());
    }

    let job: JobSpec = serde_json::from_str(&buf)?;
    let verified = if key.is_empty() {
        eprintln!("WARNING: no dispatch key (P3: envctl); treating spec as UNVERIFIED.");
        false
    } else {
        job.verify(key.as_bytes(), &sig)
            .map_err(|e| anyhow::anyhow!("signature verification failed: {e}"))?;
        true
    };

    let plan = router::route(&job);
    let place = safety::placement(&job);
    println!(
        "verified={verified} placement={place:?} kernel={:?} program={} intent='{}'",
        plan.kernel,
        plan.kernel.program(),
        plan.intent
    );
    eprintln!(
        "  P2: bind UDS, verify each frame, then invoke `{}` to delegate (never reimplement).",
        plan.kernel.program()
    );
    Ok(())
}
