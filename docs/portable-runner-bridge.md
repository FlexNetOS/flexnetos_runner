# Portable runner bridge

The portable runner bridge keeps GitHub Actions runner binaries, workspaces,
homes, generated `.path` files, and auth wiring under the release/install prefix.
Systemd is only a supervisor adapter; `/etc/systemd/system` is not the source of
truth.

## Prefix-owned state

For a prefix such as `/srv/flexnetos_runner`, the runner state remains under:

- `/srv/flexnetos_runner/_work/repos/actions-runner-01`
- `/srv/flexnetos_runner/_work/repos/actions-runner-02`
- `/srv/flexnetos_runner/_work/actions-runner-01-work`
- `/srv/flexnetos_runner/_work/actions-runner-02-work`
- `/srv/flexnetos_runner/_work/runner-home-01`
- `/srv/flexnetos_runner/_work/runner-home-02`

The installer writes `.path` files from the release/Yazelix/Nix inputs instead
of copying ambient shell history.

## Preferred: user systemd

Dry-run the install plan:

```bash
scripts/install-runner-services.sh \
  --prefix /srv/flexnetos_runner \
  --mode user \
  --dry-run
```

Apply without sudo:

```bash
scripts/install-runner-services.sh \
  --prefix /srv/flexnetos_runner \
  --mode user \
  --apply
```

This generates:

```text
~/.config/systemd/user/flexnetos-runner@.service
```

and activates:

```bash
systemctl --user daemon-reload
systemctl --user enable --now flexnetos-runner@01 flexnetos-runner@02
```

Optional root handoff for boot/login independence:

```bash
sudo loginctl enable-linger flexnetos
```

That linger command is the explicit host boundary. It is not required for the
unit files or runner state to be installed.

## Fallback: system systemd

System mode is for hosts where user systemd is unavailable or an operator wants
machine-level supervision. It generates a thin pointer unit only:

```bash
sudo scripts/install-runner-services.sh \
  --prefix /srv/flexnetos_runner \
  --mode system \
  --apply
```

Generated unit location:

```text
/etc/systemd/system/flexnetos-runner@.service
```

The unit still points back into the prefix:

```ini
ExecStart=/srv/flexnetos_runner/_work/repos/actions-runner-%i/runsvc.sh
WorkingDirectory=/srv/flexnetos_runner/_work/repos/actions-runner-%i
Environment=HOME=/srv/flexnetos_runner/_work/runner-home-%i
Environment=GIT_CONFIG_GLOBAL=/srv/flexnetos_runner/_work/runner-home-%i/.gitconfig
Environment=CODEX_HOME=...
Environment=GH_CONFIG_DIR=...
User=flexnetos
```

## Legacy retarget path

`scripts/retarget-local-runner-services.sh` is intentionally retained as the
working migration path for the currently recovered host. New portable installs
should use `scripts/install-runner-services.sh`.
