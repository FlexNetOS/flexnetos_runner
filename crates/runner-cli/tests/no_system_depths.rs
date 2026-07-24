use std::{fs, path::Path, process::Command};

#[test]
fn runner_activation_has_no_system_depth_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");

    for forbidden in [
        "nushell/runner_service.nu",
        "scripts/install-runner-services.sh",
        "scripts/cutover-to-portable-runner-services.sh",
        "scripts/retarget-local-runner-services.sh",
        ".github/workflows/runner-retarget.yml",
    ] {
        assert!(
            !root.join(forbidden).exists(),
            "NO_SYSTEM_DEPTHS violation: tracked activation surface {forbidden}"
        );
    }

    let output = Command::new("git")
        .args(["ls-files", "-co", "--exclude-standard"])
        .current_dir(root)
        .output()
        .expect("enumerate repository files");
    assert!(output.status.success(), "git ls-files failed");

    let tracked_paths = String::from_utf8(output.stdout).expect("git paths are UTF-8");
    for forbidden_prefix in ["crates/runner-actions/", "systemd/"] {
        assert!(
            !tracked_paths
                .lines()
                .any(|relative| relative.starts_with(forbidden_prefix)),
            "NO_SYSTEM_DEPTHS violation: tracked activation surface {forbidden_prefix}"
        );
    }

    let forbidden_code = [
        "systemctl",
        "loginctl",
        "systemd-run",
        "/etc/systemd",
        "enable-linger",
        "WantedBy=",
        "[Service]",
    ];

    for relative in tracked_paths.lines() {
        if relative == "crates/runner-cli/tests/no_system_depths.rs"
            || relative == "nix/gha-runner/verify.mjs"
        {
            continue;
        }
        let extension = Path::new(relative)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !matches!(
            extension,
            "rs" | "sh" | "nu" | "nix" | "yml" | "yaml" | "toml"
        ) {
            continue;
        }
        let Ok(body) = fs::read_to_string(root.join(relative)) else {
            continue;
        };
        for needle in forbidden_code {
            assert!(
                !body.contains(needle),
                "NO_SYSTEM_DEPTHS violation: {relative} contains {needle}"
            );
        }
    }
}
