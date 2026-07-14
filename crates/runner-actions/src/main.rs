//! `fxrun-actions` — self-hosted GitHub Actions runner supervisor (ADR-0008 §2).
//!
//! This binary owns the operational Actions-runner path: install the upstream runner,
//! mint short-lived org/repo registration tokens through `gh`, register an ephemeral
//! runner, and run exactly one job. FlexNetOS defaults to one org-scoped runner;
//! repo scope is an explicit exception only. Tokens are never printed.

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
use sha2::{Digest, Sha256};

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
    /// Volatile runner tree materialized from the immutable Yazelix profile.
    #[arg(
        long,
        env = "RUNNER_HOME",
        default_value = "/run/user/1001/yazelix/runners/01/runner"
    )]
    home: PathBuf,
    /// Volatile runner work directory passed to Runner.Listener.
    #[arg(
        long,
        env = "RUNNER_WORK_DIR",
        default_value = "/run/user/1001/yazelix/runners/01/work"
    )]
    work_dir: PathBuf,
    /// HOME/GIT_CONFIG_GLOBAL sandbox used by the systemd service.
    #[arg(
        long,
        env = "RUNNER_SERVICE_HOME",
        default_value = "/run/user/1001/yazelix/runners/01/home"
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
        /// Expected SHA-256 for the runner archive. If omitted, install fetches the release's
        /// .sha256 asset and still fails closed when no checksum can be obtained.
        #[arg(long, env = "RUNNER_SHA256")]
        sha256: Option<String>,
    },
    /// Register an ephemeral runner and run one job.
    RunOnce {
        /// Registration scope (defaults to org; repo is an explicit sandbox/exception).
        #[arg(long, value_enum, env = "RUNNER_SCOPE", default_value_t = Scope::Org)]
        scope: Scope,
        /// Repository name when scope=repo.
        #[arg(long, env = "RUNNER_REPO")]
        repo: Option<String>,
        /// Replace an existing local runner config.
        #[arg(long, env = "REPLACE", default_value_t = true, action = ArgAction::Set)]
        replace: bool,
    },
    /// Refused by policy: FlexNetOS runners are ephemeral and profile-supervised.
    Register {
        /// Registration scope (defaults to org; repo is an explicit sandbox/exception).
        #[arg(long, value_enum, env = "RUNNER_SCOPE", default_value_t = Scope::Org)]
        scope: Scope,
        /// Repository name when scope=repo.
        #[arg(long, env = "RUNNER_REPO")]
        repo: Option<String>,
        /// Replace an existing local runner config.
        #[arg(long, env = "REPLACE", default_value_t = true, action = ArgAction::Set)]
        replace: bool,
        /// Retained for CLI compatibility; always refused by the ephemeral-runner policy.
        #[arg(long, env = "INSTALL_SERVICE", default_value_t = false, action = ArgAction::Set)]
        service: bool,
        /// Retained for CLI compatibility; service ownership is profile-managed.
        #[arg(long, env = "RUNNER_USER", default_value = "flexnetos")]
        user: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Scope {
    Org,
    Repo,
}

#[derive(Debug, Eq, PartialEq)]
enum RegistrationKind {
    Org,
    Repo,
    Other,
}

#[derive(Debug, Eq, PartialEq)]
struct LocalRunnerConfig {
    git_hub_url: String,
    agent_name: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Doctor => doctor(&cli),
        Cmd::Install {
            version,
            arch,
            sha256,
        } => install(&cli, version.as_deref(), arch, sha256.as_deref()),
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
    println!("  canonical scope    : org (repo scope is explicit exception only)");
    println!(
        "  canonical org url  : {}",
        registration_url(&cli.org, Scope::Org, None)?
    );
    println!("  labels             : {:?}", labels_vec(&cli.labels));
    println!("  home               : {}", cli.home.display());
    println!("  work dir           : {}", cli.work_dir.display());
    println!("  service home       : {}", cli.service_home.display());
    println!(
        "  Runner.Listener    : {}",
        is_executable(runner_listener_path(&cli.home))
    );
    match read_local_runner_config(&cli.home)? {
        Some(config) => {
            let kind = classify_runner_url(&cli.org, &config.git_hub_url);
            println!(
                "  local config       : {} ({})",
                config.git_hub_url,
                registration_kind_label(&kind)
            );
            if kind == RegistrationKind::Repo {
                println!(
                    "  local config drift : repo-scoped; register an org-scoped runner before retiring this config"
                );
            }
        }
        None => println!("  local config       : unconfigured"),
    }
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

fn install(
    cli: &Cli,
    version: Option<&str>,
    arch: &str,
    expected_sha256: Option<&str>,
) -> Result<()> {
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
    let checksum_url = format!("{url}.sha256");

    if cli.dry_run {
        println!("DRY-RUN: would install actions runner v{version}");
        println!("  home: {}", cli.home.display());
        println!("  url : {url}");
        println!(
            "  sha : {}",
            expected_sha256.unwrap_or("<fetch release checksum>")
        );
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
    let expected_sha256 = match expected_sha256 {
        Some(value) => normalize_sha256(value)?,
        None => {
            let checksum_path = cli.home.join(format!("{archive}.sha256"));
            if !checksum_path.exists() {
                let status = Command::new("curl")
                    .args(["-fsSL", "-o"])
                    .arg(&checksum_path)
                    .arg(&checksum_url)
                    .status()
                    .context("download runner checksum asset")?;
                if !status.success() {
                    let _ = fs::remove_file(&checksum_path);
                }
            }
            if checksum_path.exists() {
                let checksum_text = fs::read_to_string(&checksum_path)
                    .with_context(|| format!("read {}", checksum_path.display()))?;
                parse_sha256_checksum(&checksum_text, &archive)?
            } else {
                release_notes_sha256(&version, arch, &archive)?
            }
        }
    };
    verify_file_sha256(&archive_path, &expected_sha256)?;
    run(Command::new("tar")
        .arg("xzf")
        .arg(&archive_path)
        .current_dir(&cli.home))?;

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
    run(Command::new(runner_listener_path(&cli.home))
        .arg("run")
        .current_dir(&cli.home))?;
    Ok(())
}

fn register_persistent(
    cli: &Cli,
    scope: &Scope,
    repo: Option<&str>,
    replace: bool,
    _service: bool,
    _user: &str,
) -> Result<()> {
    let _ = (cli, scope, repo, replace);
    bail!(
        "persistent runner registration is disabled: use the profile-owned Nushell service to run one ephemeral job"
    )
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
    let listener = runner_listener_path(&cli.home);
    if !is_executable(&listener) {
        bail!(
            "Runner.Listener is not executable at {}; materialize the profile-owned runner tree first",
            listener.display()
        );
    }

    warn_if_repo_scope(*scope);
    let url = registration_url(&cli.org, *scope, repo)?;
    let name = cli.name.clone().unwrap_or_else(default_name);
    let existing = read_local_runner_config(&cli.home)?;

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
        match existing {
            Some(config) => println!(
                "  existing : {} ({})",
                config.git_hub_url,
                registration_kind_label(&classify_runner_url(&cli.org, &config.git_hub_url))
            ),
            None => println!("  existing : unconfigured"),
        }
        println!("  token    : <not minted during dry-run>");
        return Ok(name);
    }

    guard_existing_runner_target(&cli.home, existing.as_ref(), &url, &cli.org)?;
    let token = mint_registration_token(&cli.org, scope, repo)?;

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
    run(Command::new(&listener)
        .arg("configure")
        .args(args)
        .current_dir(&cli.home))?;
    Ok(name)
}

fn runner_listener_path(home: &Path) -> PathBuf {
    home.join("bin/Runner.Listener")
}

fn registration_url(org: &str, scope: Scope, repo: Option<&str>) -> Result<String> {
    match scope {
        Scope::Org => Ok(format!("https://github.com/{org}")),
        Scope::Repo => {
            let repo = repo.ok_or_else(|| anyhow!("--repo is required when --scope repo"))?;
            Ok(format!("https://github.com/{org}/{repo}"))
        }
    }
}

fn read_local_runner_config(home: &Path) -> Result<Option<LocalRunnerConfig>> {
    let path = home.join(".runner");
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let text = text.trim_start_matches('\u{feff}');
    let json: serde_json::Value = serde_json::from_str(text)
        .with_context(|| format!("parse {} as runner config JSON", path.display()))?;
    let git_hub_url = json
        .get("gitHubUrl")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("{} is missing gitHubUrl", path.display()))?
        .to_string();
    let agent_name = json
        .get("agentName")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    Ok(Some(LocalRunnerConfig {
        git_hub_url,
        agent_name,
    }))
}

fn classify_runner_url(org: &str, url: &str) -> RegistrationKind {
    let org_url = format!("https://github.com/{org}");
    if url == org_url {
        return RegistrationKind::Org;
    }
    if let Some(rest) = url.strip_prefix(&(org_url + "/")) {
        if !rest.is_empty() && !rest.contains('/') {
            return RegistrationKind::Repo;
        }
    }
    RegistrationKind::Other
}

fn registration_kind_label(kind: &RegistrationKind) -> &'static str {
    match kind {
        RegistrationKind::Org => "org-scoped",
        RegistrationKind::Repo => "repo-scoped",
        RegistrationKind::Other => "non-FlexNetOS/unknown",
    }
}

fn warn_if_repo_scope(scope: Scope) {
    if scope == Scope::Repo {
        eprintln!(
            "WARN: repo-scoped GitHub Actions runners are an explicit exception; \
             FlexNetOS production/default is one org-scoped runner shared by meta peers."
        );
    }
}

fn guard_existing_runner_target(
    home: &Path,
    existing: Option<&LocalRunnerConfig>,
    target_url: &str,
    org: &str,
) -> Result<()> {
    let Some(existing) = existing else {
        return Ok(());
    };
    if existing.git_hub_url == target_url {
        return Ok(());
    }

    let existing_kind = registration_kind_label(&classify_runner_url(org, &existing.git_hub_url));
    let target_kind = registration_kind_label(&classify_runner_url(org, target_url));
    bail!(
        "runner home {} is already configured for {} ({existing_kind}); refusing to mutate it \
         in-place to {target_url} ({target_kind}). Strict upgrade path: register the org-scoped \
         runner in a clean RUNNER_HOME while the old runner remains available, verify FlexNetOS/meta \
         and FlexNetOS/envctl consume the shared labels, then retire the repo-scoped service/config.",
        home.display(),
        existing.git_hub_url
    )
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

fn normalize_sha256(value: &str) -> Result<String> {
    let hash = value
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("empty SHA-256 checksum"))?
        .trim()
        .trim_start_matches("sha256:")
        .trim_start_matches("sha256=")
        .to_ascii_lowercase();
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid SHA-256 checksum `{value}`");
    }
    Ok(hash)
}

fn parse_sha256_checksum(text: &str, archive: &str) -> Result<String> {
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let filename = parts.next().unwrap_or(archive).trim_start_matches('*');
        if filename == archive || parts.next().is_none() {
            return normalize_sha256(hash);
        }
    }
    bail!("checksum file did not contain SHA-256 for {archive}")
}

fn parse_release_notes_sha256(text: &str, arch: &str, archive: &str) -> Result<String> {
    let marker = format!("BEGIN SHA {arch}");
    for line in text.lines().map(str::trim) {
        if !(line.contains(archive) || line.contains(&marker)) {
            continue;
        }
        for candidate in line.split(|c: char| !c.is_ascii_hexdigit()) {
            if candidate.len() == 64 && candidate.chars().all(|c| c.is_ascii_hexdigit()) {
                return normalize_sha256(candidate);
            }
        }
    }
    bail!("release notes did not contain SHA-256 for {archive}")
}

fn release_notes_sha256(version: &str, arch: &str, archive: &str) -> Result<String> {
    require_cmd("gh")?;
    let tag = format!("v{}", version.trim_start_matches('v'));
    let out = Command::new("gh")
        .args([
            "release",
            "view",
            &tag,
            "--repo",
            "actions/runner",
            "--json",
            "body",
            "--jq",
            ".body",
        ])
        .output()
        .with_context(|| format!("query actions/runner {tag} release notes"))?;
    if !out.status.success() {
        bail!(
            "gh release notes query failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let body = String::from_utf8(out.stdout)?;
    parse_release_notes_sha256(&body, arch, archive)
}

fn verify_file_sha256(path: &Path, expected: &str) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        bail!(
            "SHA-256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        );
    }
    Ok(())
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
    fn checksum_parser_accepts_sha256sum_formats_and_rejects_invalid_hashes() {
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(normalize_sha256(hash).unwrap(), hash);
        assert_eq!(normalize_sha256(&format!("sha256={hash}")).unwrap(), hash);
        assert_eq!(
            parse_sha256_checksum(
                &format!("{hash}  actions-runner-linux-x64-2.329.0.tar.gz\n"),
                "actions-runner-linux-x64-2.329.0.tar.gz"
            )
            .unwrap(),
            hash
        );
        assert_eq!(
            parse_sha256_checksum(
                &format!("{hash} *actions-runner-linux-x64-2.329.0.tar.gz\n"),
                "actions-runner-linux-x64-2.329.0.tar.gz"
            )
            .unwrap(),
            hash
        );
        assert!(normalize_sha256("not-a-sha").is_err());
    }

    #[test]
    fn release_notes_parser_accepts_runner_sha_comments() {
        let hash = "4ef2f25285f0ae4477f1fe1e346db76d2f3ebf03824e2ddd1973a2819bf6c8cf";
        let body = format!(
            "- actions-runner-linux-x64-2.335.1.tar.gz <!-- BEGIN SHA linux-x64 -->{hash}<!-- END SHA linux-x64 -->\n"
        );

        assert_eq!(
            parse_release_notes_sha256(
                &body,
                "linux-x64",
                "actions-runner-linux-x64-2.335.1.tar.gz"
            )
            .unwrap(),
            hash
        );
        assert!(parse_release_notes_sha256(
            &body,
            "linux-arm64",
            "actions-runner-linux-arm64-2.335.1.tar.gz"
        )
        .is_err());
    }

    #[test]
    fn verify_file_sha256_detects_mismatch() {
        let path = env::temp_dir().join(format!("fxrun-actions-sha-test-{}", std::process::id()));
        fs::write(&path, b"runner").unwrap();
        let actual = hex::encode(Sha256::digest(b"runner"));
        verify_file_sha256(&path, &actual).unwrap();
        assert!(verify_file_sha256(
            &path,
            "0000000000000000000000000000000000000000000000000000000000000000"
        )
        .is_err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn registration_scope_defaults_to_org() {
        let cli = Cli::try_parse_from(["fxrun-actions", "register"]).unwrap();
        match cli.cmd {
            Cmd::Register { scope, repo, .. } => {
                assert_eq!(scope, Scope::Org);
                assert_eq!(repo, None);
            }
            _ => panic!("expected register command"),
        }

        let cli = Cli::try_parse_from(["fxrun-actions", "run-once"]).unwrap();
        match cli.cmd {
            Cmd::RunOnce { scope, repo, .. } => {
                assert_eq!(scope, Scope::Org);
                assert_eq!(repo, None);
            }
            _ => panic!("expected run-once command"),
        }
    }

    #[test]
    fn runner_lifecycle_uses_listener_elf_and_refuses_persistent_registration() {
        assert_eq!(
            runner_listener_path(Path::new("/runner")),
            PathBuf::from("/runner/bin/Runner.Listener")
        );

        let cli = Cli::try_parse_from(["fxrun-actions", "register"]).unwrap();
        let Cmd::Register {
            scope,
            repo,
            replace,
            service,
            user,
        } = &cli.cmd
        else {
            panic!("expected register command");
        };
        let error = register_persistent(
            &cli,
            scope,
            repo.as_deref(),
            *replace,
            *service,
            user,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("persistent runner registration is disabled"));
        assert!(error.contains("profile-owned Nushell service"));
    }

    #[test]
    fn registration_url_requires_repo_only_for_repo_scope() {
        assert_eq!(
            registration_url("FlexNetOS", Scope::Org, None).unwrap(),
            "https://github.com/FlexNetOS"
        );
        assert_eq!(
            registration_url("FlexNetOS", Scope::Repo, Some("envctl")).unwrap(),
            "https://github.com/FlexNetOS/envctl"
        );
        assert!(registration_url("FlexNetOS", Scope::Repo, None).is_err());
    }

    #[test]
    fn classifies_flexnetos_runner_urls() {
        assert_eq!(
            classify_runner_url("FlexNetOS", "https://github.com/FlexNetOS"),
            RegistrationKind::Org
        );
        assert_eq!(
            classify_runner_url("FlexNetOS", "https://github.com/FlexNetOS/envctl"),
            RegistrationKind::Repo
        );
        assert_eq!(
            classify_runner_url("FlexNetOS", "https://github.com/Other/envctl"),
            RegistrationKind::Other
        );
        assert_eq!(
            classify_runner_url("FlexNetOS", "https://github.com/FlexNetOS/envctl/extra"),
            RegistrationKind::Other
        );
    }

    #[test]
    fn local_runner_config_reads_github_url() {
        let dir = env::temp_dir().join(format!(
            "fxrun-actions-runner-config-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(".runner"),
            "\u{feff}{\"gitHubUrl\":\"https://github.com/FlexNetOS/envctl\",\"agentName\":\"fxrun\"}",
        )
        .unwrap();

        assert_eq!(
            read_local_runner_config(&dir).unwrap(),
            Some(LocalRunnerConfig {
                git_hub_url: "https://github.com/FlexNetOS/envctl".into(),
                agent_name: Some("fxrun".into()),
            })
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_runner_target_guard_refuses_scope_mutation() {
        let config = LocalRunnerConfig {
            git_hub_url: "https://github.com/FlexNetOS/envctl".into(),
            agent_name: None,
        };

        assert!(guard_existing_runner_target(
            Path::new("/runner"),
            Some(&config),
            "https://github.com/FlexNetOS/envctl",
            "FlexNetOS"
        )
        .is_ok());
        let err = guard_existing_runner_target(
            Path::new("/runner"),
            Some(&config),
            "https://github.com/FlexNetOS",
            "FlexNetOS",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("refusing to mutate it in-place"));
        assert!(err.contains("Strict upgrade path"));
    }

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
