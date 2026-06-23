//! `fxrun-actions` — self-hosted GitHub Actions runner supervisor (ADR-0008 §2).
//!
//! This binary owns the operational Actions-runner path: install the upstream runner,
//! mint short-lived repo/org registration tokens through `gh`, register an ephemeral
//! runner, and run exactly one job. Tokens are never printed.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, bail, Context, Result};
use clap::{ArgAction, Parser, Subcommand, ValueEnum};
use runner_core::{
    lifecycle::{JitConfigRequest, State},
    safety::Rails,
};

#[derive(Debug, Parser)]
#[command(
    name = "fxrun-actions",
    version,
    about = "FlexNetOS GitHub Actions runner supervisor"
)]
struct Cli {
    /// GitHub owner or organization.
    #[arg(long, env = "RUNNER_ORG", default_value = "FlexNetOS")]
    org: String,
    /// Runner install directory.
    #[arg(
        long,
        env = "RUNNER_HOME",
        default_value = "/home/drdave/_work/repos/actions-runner"
    )]
    home: PathBuf,
    /// Runner work directory passed to config.sh.
    #[arg(
        long,
        env = "RUNNER_WORK_DIR",
        default_value = "/home/drdave/_work/actions-runner-work"
    )]
    work_dir: PathBuf,
    /// HOME/GIT_CONFIG_GLOBAL sandbox used by the systemd service.
    #[arg(
        long,
        env = "RUNNER_SERVICE_HOME",
        default_value = "/home/drdave/_work/runner-home"
    )]
    service_home: PathBuf,
    /// Comma-separated runner labels.
    #[arg(
        long,
        env = "RUNNER_LABELS",
        default_value = "self-hosted,linux,x64,local,flexnetos"
    )]
    labels: String,
    /// Runner name.
    #[arg(long, env = "RUNNER_NAME")]
    name: Option<String>,
    /// Print intended actions only.
    #[arg(long, env = "DRY_RUN", default_value_t = true, action = ArgAction::Set)]
    dry_run: bool,
    /// Required for host or GitHub mutations.
    #[arg(long, env = "CONFIRM", default_value_t = false, action = ArgAction::Set)]
    confirm: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Read-only readiness report.
    Doctor,
    /// Install or upgrade the upstream actions/runner binaries.
    Install {
        /// Version without leading v. Empty means latest release from GitHub.
        #[arg(long, env = "RUNNER_VERSION")]
        version: Option<String>,
        /// Runner architecture.
        #[arg(long, env = "RUNNER_ARCH", default_value = "x64")]
        arch: String,
    },
    /// Register an ephemeral runner and run one job.
    RunOnce {
        /// Registration scope.
        #[arg(long, value_enum, env = "RUNNER_SCOPE", default_value_t = Scope::Repo)]
        scope: Scope,
        /// Repository name when scope=repo.
        #[arg(long, env = "RUNNER_REPO")]
        repo: Option<String>,
        /// Replace an existing local runner config.
        #[arg(long, env = "REPLACE", default_value_t = true, action = ArgAction::Set)]
        replace: bool,
    },
    /// Register a persistent runner, optionally installed as a system service.
    Register {
        /// Registration scope.
        #[arg(long, value_enum, env = "RUNNER_SCOPE", default_value_t = Scope::Repo)]
        scope: Scope,
        /// Repository name when scope=repo.
        #[arg(long, env = "RUNNER_REPO")]
        repo: Option<String>,
        /// Replace an existing local runner config.
        #[arg(long, env = "REPLACE", default_value_t = true, action = ArgAction::Set)]
        replace: bool,
        /// Install and start the GitHub runner service after registration.
        #[arg(long, env = "INSTALL_SERVICE", default_value_t = false, action = ArgAction::Set)]
        service: bool,
        /// User for svc.sh install.
        #[arg(long, env = "RUNNER_USER", default_value = "drdave")]
        user: String,
    },
}

#[derive(Clone, Debug, ValueEnum)]
enum Scope {
    Org,
    Repo,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Doctor => doctor(&cli),
        Cmd::Install { version, arch } => install(&cli, version.as_deref(), arch),
        Cmd::RunOnce {
            scope,
            repo,
            replace,
        } => run_once(&cli, scope, repo.as_deref(), *replace),
        Cmd::Register {
            scope,
            repo,
            replace,
            service,
            user,
        } => register_persistent(&cli, scope, repo.as_deref(), *replace, *service, user),
    }
}

fn doctor(cli: &Cli) -> Result<()> {
    let rails = Rails::default();
    let req = JitConfigRequest::new(default_name(), 0, labels_vec(&cli.labels));
    println!("fxrun-actions");
    println!("  rails safe         : {}", rails.is_safe());
    println!("  labels             : {:?}", labels_vec(&cli.labels));
    println!("  home               : {}", cli.home.display());
    println!("  work dir           : {}", cli.work_dir.display());
    println!("  service home       : {}", cli.service_home.display());
    println!(
        "  config.sh          : {}",
        cli.home.join("config.sh").is_file()
    );
    println!(
        "  run.sh             : {}",
        cli.home.join("run.sh").is_file()
    );
    println!("  gh                 : {}", has_cmd("gh"));
    println!("  curl               : {}", has_cmd("curl"));
    println!("  tar                : {}", has_cmd("tar"));
    print!("  lifecycle          :");
    let mut state = State::Unregistered;
    print!(" {state:?}");
    while let Some(next) = state.next() {
        print!(" -> {next:?}");
        state = next;
    }
    println!();
    println!("  jit request shape  : {:?}", req);
    Ok(())
}

fn install(cli: &Cli, version: Option<&str>, arch: &str) -> Result<()> {
    require_confirm(cli)?;
    require_cmd("curl")?;
    require_cmd("tar")?;

    let version = match version.filter(|v| !v.is_empty()) {
        Some(version) => version.trim_start_matches('v').to_string(),
        None => latest_runner_version()?,
    };
    // GitHub enforces a minimum self-hosted runner version: registration is refused and job
    // queuing pauses below it, and pre-floor runners carry the "Runner-Escape" host-env/SSH-key
    // exposure (github.blog changelog 2026-06-12). Fail closed on an explicitly-pinned stale
    // version rather than installing a runner GitHub will reject.
    if !meets_min_runner_version(&version) {
        bail!(
            "actions/runner v{version} is below the enforced minimum v{MIN_RUNNER_VERSION} \
             (GitHub refuses registration / pauses job queuing below it; pre-floor runners are \
             exposed to the Runner-Escape host-secret leak). Omit --version to fetch the latest."
        );
    }
    let archive = format!("actions-runner-linux-{arch}-{version}.tar.gz");
    let url = format!("https://github.com/actions/runner/releases/download/v{version}/{archive}");

    if cli.dry_run {
        println!("DRY-RUN: would install actions runner v{version}");
        println!("  home: {}", cli.home.display());
        println!("  url : {url}");
        return Ok(());
    }

    fs::create_dir_all(&cli.home).with_context(|| format!("create {}", cli.home.display()))?;
    let archive_path = cli.home.join(&archive);
    if !archive_path.exists() {
        run(Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&archive_path)
            .arg(&url))?;
    }
    run(Command::new("tar")
        .arg("xzf")
        .arg(&archive_path)
        .current_dir(&cli.home))?;

    let deps = cli.home.join("bin/installdependencies.sh");
    if deps.is_file() {
        let status = Command::new("sudo")
            .arg(&deps)
            .current_dir(&cli.home)
            .status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => eprintln!("WARN: dependency installer exited with {status}"),
            Err(err) => eprintln!("WARN: could not run dependency installer: {err}"),
        }
    }
    println!(
        "OK: actions runner v{version} installed at {}",
        cli.home.display()
    );
    Ok(())
}

fn run_once(cli: &Cli, scope: &Scope, repo: Option<&str>, replace: bool) -> Result<()> {
    let _name = configure_runner(cli, scope, repo, replace, true)?;
    if cli.dry_run {
        return Ok(());
    }
    println!("OK: runner registered; running exactly one job");
    run(Command::new(cli.home.join("run.sh")).current_dir(&cli.home))?;
    Ok(())
}

fn register_persistent(
    cli: &Cli,
    scope: &Scope,
    repo: Option<&str>,
    replace: bool,
    service: bool,
    user: &str,
) -> Result<()> {
    let name = configure_runner(cli, scope, repo, replace, false)?;
    if cli.dry_run {
        return Ok(());
    }
    if service {
        let svc = cli.home.join("svc.sh");
        if !svc.is_file() {
            bail!("{} is missing; cannot install service", svc.display());
        }
        run(Command::new("sudo")
            .arg(&svc)
            .arg("install")
            .arg(user)
            .current_dir(&cli.home))?;
        let unit = service_unit_name(&cli.org, scope, repo, &name)?;
        install_service_home_dropin(&unit, &cli.service_home)?;
        run(Command::new("sudo")
            .arg(&svc)
            .arg("start")
            .current_dir(&cli.home))?;
        let _ = Command::new("sudo")
            .arg(&svc)
            .arg("status")
            .current_dir(&cli.home)
            .status();
    } else {
        println!(
            "OK: runner registered. Start it with {}",
            cli.home.join("run.sh").display()
        );
    }
    Ok(())
}

fn configure_runner(
    cli: &Cli,
    scope: &Scope,
    repo: Option<&str>,
    replace: bool,
    ephemeral: bool,
) -> Result<String> {
    require_confirm(cli)?;
    require_cmd("gh")?;
    let config = cli.home.join("config.sh");
    if !config.is_file() || !cli.home.join("run.sh").is_file() {
        bail!(
            "runner is not installed at {}; run `fxrun-actions install --dry-run=false --confirm=true` first",
            cli.home.display()
        );
    }

    let url = match scope {
        Scope::Org => format!("https://github.com/{}", cli.org),
        Scope::Repo => {
            let repo = repo.ok_or_else(|| anyhow!("--repo is required when --scope repo"))?;
            format!("https://github.com/{}/{}", cli.org, repo)
        }
    };
    let name = cli.name.clone().unwrap_or_else(default_name);
    let token = mint_registration_token(&cli.org, scope, repo)?;

    if cli.dry_run {
        let kind = if ephemeral {
            "ephemeral runner and run one job"
        } else {
            "persistent runner"
        };
        println!("DRY-RUN: would register {kind}");
        println!("  url      : {url}");
        println!("  home     : {}", cli.home.display());
        println!("  work dir : {}", cli.work_dir.display());
        println!("  svc home : {}", cli.service_home.display());
        println!("  name     : {name}");
        println!("  labels   : {}", cli.labels);
        return Ok(name);
    }

    fs::create_dir_all(&cli.work_dir)
        .with_context(|| format!("create {}", cli.work_dir.display()))?;
    let mut args = vec![
        "--url".to_string(),
        url,
        "--token".to_string(),
        token,
        "--labels".to_string(),
        cli.labels.clone(),
        "--name".to_string(),
        name.clone(),
        "--work".to_string(),
        cli.work_dir.to_string_lossy().to_string(),
        "--unattended".to_string(),
    ];
    if ephemeral {
        args.push("--ephemeral".to_string());
    }
    if replace {
        args.push("--replace".to_string());
    }
    run(Command::new(&config).args(args).current_dir(&cli.home))?;
    Ok(name)
}

fn service_unit_name(org: &str, scope: &Scope, repo: Option<&str>, name: &str) -> Result<String> {
    let scope_part = match scope {
        Scope::Org => org.to_string(),
        Scope::Repo => format!(
            "{org}-{}",
            repo.ok_or_else(|| anyhow!("--repo is required when --scope repo"))?
        ),
    };
    Ok(format!("actions.runner.{scope_part}.{name}.service"))
}

fn install_service_home_dropin(unit: &str, service_home: &Path) -> Result<()> {
    fs::create_dir_all(service_home)
        .with_context(|| format!("create runner service home {}", service_home.display()))?;
    let gitconfig = service_home.join(".gitconfig");
    if !gitconfig.exists() {
        fs::write(
            &gitconfig,
            "[credential \"https://github.com\"]\n\thelper = \n\thelper = !/usr/bin/gh auth git-credential\n",
        )
        .with_context(|| format!("write {}", gitconfig.display()))?;
    }

    let dropin_dir = format!("/etc/systemd/system/{unit}.d");
    let dropin_content = format!(
        "[Service]\nEnvironment=HOME={home}\nEnvironment=GIT_CONFIG_GLOBAL={home}/.gitconfig\n",
        home = service_home.display()
    );
    let tmp = env::temp_dir().join(format!("fxrun-actions-{unit}-runner-home.conf"));
    fs::write(&tmp, dropin_content).with_context(|| format!("write {}", tmp.display()))?;
    run(Command::new("sudo").args(["mkdir", "-p", &dropin_dir]))?;
    run(Command::new("sudo")
        .arg("install")
        .args(["-m", "0644"])
        .arg(&tmp)
        .arg(format!("{dropin_dir}/10-runner-home.conf")))?;
    let _ = fs::remove_file(&tmp);
    run(Command::new("sudo").args(["systemctl", "daemon-reload"]))?;
    Ok(())
}

/// GitHub-enforced minimum self-hosted `actions/runner` version (changelog 2026-06-12). Below
/// this, GitHub refuses registration and pauses job queuing, and the runner is exposed to the
/// Runner-Escape host-secret leak. The supervisor will not install a version under this floor.
const MIN_RUNNER_VERSION: &str = "2.329.0";

/// Parse a dotted `major.minor.patch` (extra/non-numeric components ignored) for comparison.
/// Fail-open on an unparseable version (treated as "meets floor") so a future tag format we
/// don't recognize never blocks a legitimately-latest runner — the floor exists to catch
/// *explicitly-pinned stale* versions, not to second-guess GitHub's own latest tag.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it
        .next()
        .unwrap_or("0")
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("0")
        .parse()
        .ok()?;
    Some((major, minor, patch))
}

/// Whether `version` meets [`MIN_RUNNER_VERSION`]. Unparseable inputs fail open (see
/// [`parse_version`]).
fn meets_min_runner_version(version: &str) -> bool {
    match (parse_version(version), parse_version(MIN_RUNNER_VERSION)) {
        (Some(v), Some(min)) => v >= min,
        _ => true,
    }
}

fn latest_runner_version() -> Result<String> {
    require_cmd("gh")?;
    let out = Command::new("gh")
        .args([
            "api",
            "repos/actions/runner/releases/latest",
            "--jq",
            ".tag_name",
        ])
        .output()
        .context("query latest actions/runner release")?;
    if !out.status.success() {
        bail!(
            "gh release query failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let tag = String::from_utf8(out.stdout)?
        .trim()
        .trim_start_matches('v')
        .to_string();
    if tag.is_empty() {
        bail!("latest actions/runner release tag was empty");
    }
    Ok(tag)
}

fn mint_registration_token(org: &str, scope: &Scope, repo: Option<&str>) -> Result<String> {
    let endpoint = match scope {
        Scope::Org => format!("/orgs/{org}/actions/runners/registration-token"),
        Scope::Repo => format!(
            "/repos/{org}/{}/actions/runners/registration-token",
            repo.ok_or_else(|| anyhow!("--repo is required when --scope repo"))?
        ),
    };
    let out = Command::new("gh")
        .args(["api", "-X", "POST", &endpoint, "--jq", ".token"])
        .output()
        .with_context(|| format!("mint runner registration token at {endpoint}"))?;
    if !out.status.success() {
        bail!(
            "gh token mint failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let token = String::from_utf8(out.stdout)?.trim().to_string();
    if token.is_empty() {
        bail!("GitHub returned an empty runner registration token");
    }
    Ok(token)
}

fn require_confirm(cli: &Cli) -> Result<()> {
    if !cli.dry_run && !cli.confirm {
        bail!("refusing host/GitHub mutation without --confirm=true");
    }
    Ok(())
}

fn run(cmd: &mut Command) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().to_string();
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("run {program}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

fn require_cmd(cmd: &str) -> Result<()> {
    if has_cmd(cmd) {
        Ok(())
    } else {
        bail!("{cmd} is required")
    }
}

fn has_cmd(cmd: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|path| is_executable(path.join(cmd))))
        .unwrap_or(false)
}

fn is_executable(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    path.is_file()
        && fs::metadata(path)
            .map(|m| !m.permissions().readonly())
            .unwrap_or(false)
}

fn labels_vec(labels: &str) -> Vec<String> {
    labels
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn default_name() -> String {
    let host = Command::new("hostname")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    format!("fxrun-{host}-{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_version_floor_is_enforced() {
        // At or above the floor → allowed.
        assert!(meets_min_runner_version(MIN_RUNNER_VERSION));
        assert!(meets_min_runner_version("2.329.0"));
        assert!(meets_min_runner_version("2.330.1"));
        assert!(meets_min_runner_version("v2.329.0")); // leading-v tolerated
        assert!(meets_min_runner_version("3.0.0"));

        // Below the floor → refused.
        assert!(!meets_min_runner_version("2.328.0"));
        assert!(!meets_min_runner_version("2.300.0"));
        assert!(!meets_min_runner_version("1.999.999"));
    }

    #[test]
    fn parse_version_is_tolerant_and_fails_open() {
        assert_eq!(parse_version("2.329.0"), Some((2, 329, 0)));
        assert_eq!(parse_version("2.329"), Some((2, 329, 0)));
        assert_eq!(parse_version("2.329.0-rc1"), Some((2, 329, 0)));
        // Garbage is unparseable → floor check fails open (never blocks a real latest tag).
        assert_eq!(parse_version("not-a-version"), None);
        assert!(meets_min_runner_version("not-a-version"));
    }
}
