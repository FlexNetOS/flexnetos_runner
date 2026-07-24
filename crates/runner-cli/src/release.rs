//! `fxrun release` — first-class local release-compile lane. Shells to
//! `scripts/build-local-ubuntu-release.sh` with runner-local defaults pre-wired so the local
//! LOCAL lane is the default: a runner-local `cargo` (and `bun`) is resolved automatically, so
//! `FXRUN_CARGO=` is not required on the command line. `release check` maps to the script's
//! `--check-only`; `release build` runs the full compile+stage+tar lane.
//!
//! This is additive: every wired default is still overridable through the same environment
//! variables the script already honors, and the public GitHub lane is untouched.

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Workspace-root-relative location of the local release script.
const SCRIPT_REL: &str = "flexnetos_runner/scripts/build-local-ubuntu-release.sh";

#[derive(Subcommand)]
pub enum ReleaseCommand {
    /// Compile the catalog, stage provenance + proof, and write the local release tarball.
    Build(ReleaseArgs),
    /// Validate host, toolchain, and catalog wiring without compiling (script `--check-only`).
    Check(ReleaseArgs),
}

#[derive(Args, Clone, Debug, Default)]
pub struct ReleaseArgs {
    /// Workspace root. Default: FXRUN_WORKSPACE_ROOT / FLEXNETOS_ROOT env, else discovered from the
    /// running binary or the current directory, else the canonical workspace root.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Release output root passed to the script (`--out`). Default: <root>/release.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Restrict the build/check to these catalog components (comma- or space-separated).
    #[arg(long)]
    pub components: Option<String>,
    /// Cargo binary override. Default: runner-local toolchain cargo, else PATH cargo.
    #[arg(long)]
    pub cargo: Option<PathBuf>,
    /// Bun binary override. Default: workspace toolchain bun, else PATH bun.
    #[arg(long)]
    pub bun: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Build,
    Check,
}

/// Fully resolved invocation, kept pure so it can be asserted in tests without executing.
#[derive(Debug, Clone)]
struct Prepared {
    script: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    out_root: PathBuf,
}

pub fn execute(cmd: ReleaseCommand) -> Result<()> {
    let (mode, args) = match cmd {
        ReleaseCommand::Build(a) => (Mode::Build, a),
        ReleaseCommand::Check(a) => (Mode::Check, a),
    };
    let prepared = prepare(mode, &args)?;

    let label = if mode == Mode::Check {
        "check"
    } else {
        "build"
    };
    println!("fxrun release {label}");
    println!("  script   : {}", prepared.script.display());
    println!("  out_root : {}", prepared.out_root.display());
    for (k, v) in &prepared.env {
        println!("  env      : {k}={v}");
    }

    let mut command = Command::new("bash");
    command.arg(&prepared.script).args(&prepared.args);
    for (k, v) in &prepared.env {
        command.env(k, v);
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn {}", prepared.script.display()))?;
    if !status.success() {
        bail!(
            "release {label} failed: {} exited with {}",
            prepared.script.display(),
            status
        );
    }
    Ok(())
}

fn prepare(mode: Mode, args: &ReleaseArgs) -> Result<Prepared> {
    let root = resolve_root(args.root.as_deref())?;
    let script = root.join(SCRIPT_REL);
    if !script.is_file() {
        bail!(
            "release script not found at {} (set --root or FXRUN_WORKSPACE_ROOT)",
            script.display()
        );
    }
    let out_root = args.out.clone().unwrap_or_else(|| default_out_root(&root));

    let mut script_args: Vec<String> = Vec::new();
    if mode == Mode::Check {
        script_args.push("--check-only".to_string());
    }
    if let Some(out) = &args.out {
        script_args.push("--out".to_string());
        script_args.push(out.display().to_string());
    }

    let mut env: Vec<(String, String)> = Vec::new();
    if let Some(cargo) = resolve_cargo(&root, args.cargo.as_deref()) {
        env.push(("FXRUN_CARGO".to_string(), cargo.display().to_string()));
    }
    if let Some(bun) = resolve_bun(&root, args.bun.as_deref()) {
        env.push(("FXRUN_BUN".to_string(), bun.display().to_string()));
    }
    if let Some(components) = &args.components {
        let normalized = components
            .split([',', ' '])
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        env.push(("FXRUN_RELEASE_COMPONENTS".to_string(), normalized));
    }

    Ok(Prepared {
        script,
        args: script_args,
        env,
        out_root,
    })
}

/// Default release output root: `<root>/release`. Mirrors the script default so the tarball and
/// staging tree land deterministically alongside the workspace release directory.
fn default_out_root(root: &Path) -> PathBuf {
    root.join("release")
}

fn resolve_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return Ok(canonical_or_owned(root));
    }
    for var in ["FXRUN_WORKSPACE_ROOT", "FLEXNETOS_ROOT"] {
        if let Ok(value) = std::env::var(var) {
            if !value.is_empty() {
                return Ok(canonical_or_owned(Path::new(&value)));
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = discover_root_from(&exe) {
            return Ok(root);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = discover_root_from(&cwd) {
            return Ok(root);
        }
    }
    for candidate in ["/home/flexnetos/meta", "/home/flexnetos/FlexNetOS"] {
        let path = Path::new(candidate);
        if path.join(SCRIPT_REL).is_file() {
            return Ok(canonical_or_owned(path));
        }
    }
    bail!("could not resolve the workspace root; pass --root or set FXRUN_WORKSPACE_ROOT")
}

/// Walk up from `start` looking for an ancestor that contains the release script.
fn discover_root_from(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.join(SCRIPT_REL).is_file() {
            return Some(canonical_or_owned(ancestor));
        }
    }
    None
}

fn resolve_cargo(root: &Path, explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(cargo) = explicit {
        return Some(cargo.to_path_buf());
    }
    if let Ok(value) = std::env::var("FXRUN_CARGO") {
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    for slot in ["runner-home-02", "runner-home-01"] {
        let cargo = root.join(format!(
            "flexnetos_runner/_work/{slot}/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo"
        ));
        if is_executable(&cargo) {
            return Some(cargo);
        }
    }
    which("cargo")
}

fn resolve_bun(root: &Path, explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(bun) = explicit {
        return Some(bun.to_path_buf());
    }
    if let Ok(value) = std::env::var("FXRUN_BUN") {
        if !value.is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    let bun = root.join(".toolchains/.bun/bin/bun");
    if is_executable(&bun) {
        return Some(bun);
    }
    which("bun")
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn canonical_or_owned(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_out_root_is_release_under_workspace_root() {
        assert_eq!(
            default_out_root(Path::new("/home/flexnetos/meta")),
            PathBuf::from("/home/flexnetos/meta/release")
        );
    }

    #[test]
    fn release_check_invokes_script_with_check_only() {
        let root = temp_root_with_script("check-only");
        let args = ReleaseArgs {
            root: Some(root.clone()),
            ..Default::default()
        };
        let prepared = prepare(Mode::Check, &args).expect("prepare check");

        assert!(
            prepared.script.ends_with(SCRIPT_REL),
            "script path must be the release script: {}",
            prepared.script.display()
        );
        assert!(
            prepared.args.iter().any(|a| a == "--check-only"),
            "check must pass --check-only, got {:?}",
            prepared.args
        );
        assert_eq!(
            prepared.out_root,
            default_out_root(&canonical_or_owned(&root))
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn release_build_omits_check_only() {
        let root = temp_root_with_script("build");
        let args = ReleaseArgs {
            root: Some(root.clone()),
            ..Default::default()
        };
        let prepared = prepare(Mode::Build, &args).expect("prepare build");
        assert!(
            !prepared.args.iter().any(|a| a == "--check-only"),
            "build must not pass --check-only, got {:?}",
            prepared.args
        );
        std::fs::remove_dir_all(&root).ok();
    }

    fn temp_root_with_script(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "fxrun-release-test-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let script = root.join(SCRIPT_REL);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "#!/usr/bin/env bash\nexit 0\n").unwrap();
        root
    }
}
