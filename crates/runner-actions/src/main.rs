//! `fxrun-actions` — self-hosted GitHub Actions runner supervisor (ADR-0008 §2).
//!
//! P0 is a dry-run skeleton: it constructs the JIT config request and reports the rails it
//! WILL enforce + the ephemeral lifecycle it WILL drive. The live
//! `POST /orgs/{org}/actions/runners/generate-jitconfig` call, agent launch with the
//! returned `encoded_jit_config`, single-job supervision, and deregistration land in P1.
//! Productizes the shell in `.github_org/runner/{ephemeral-spawn,register,remove}.sh`.

use runner_core::{
    lifecycle::{JitConfigRequest, State},
    safety::Rails,
};

fn main() {
    let rails = Rails::default();
    let req = JitConfigRequest::new("fxrun-ephemeral", 0, rails.labels.clone());

    eprintln!("fxrun-actions P0 (dry-run)");
    eprintln!("  rails  : {rails:?}");
    eprintln!("  safe   : {}", rails.is_safe());
    eprintln!("  jitcfg : {req:?}");
    eprint!("  cycle  :");
    let mut s = State::Unregistered;
    eprint!(" {s:?}");
    while let Some(n) = s.next() {
        eprint!(" -> {n:?}");
        s = n;
    }
    eprintln!();
    eprintln!(
        "  P1: POST generate-jitconfig, launch agent with encoded_jit_config, supervise one \
         job, deregister; fork PRs are refused on self-hosted (see runner_core::safety)."
    );
}
