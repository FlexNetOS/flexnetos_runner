# Git Upstream Worktree Sync Template

Reusable FlexNetOS template for syncing a fork with an upstream remote through
an isolated git worktree. This preserves local work in the main checkout while
making the upstream merge explicit, inspectable, and easy to land directly when
branch policy allows.

This template was distilled from the `rtk-tokenkill` upstream sync pattern:

- merge commit: `9e6f7b0774b781261422f31f7cefe5607f449595`
- upstream worktree commands source: `ab83e7933ebc26ca76f843d33285729875efb913`
- observed sync branch: `chore/sync-upstream-0.42.4`
- observed remote-tracking merge: `upstream/master`

## Contents

```text
templates/git-upstream-worktree-sync/
|-- README.md
|-- template.toml
|-- .gitignore.append
|-- .worktreeinclude.example
|-- scripts/
|   `-- sync-upstream-worktree.sh
`-- claude/
    `-- commands/
        |-- sync-upstream.md
        |-- worktree.md
        |-- worktree-status.md
        `-- clean-worktrees.md
```

## Install In A Repo

From the target repo:

```bash
mkdir -p scripts .claude/commands
cp /home/flexnetos/meta/flexnetos_runner/templates/git-upstream-worktree-sync/scripts/sync-upstream-worktree.sh scripts/
cp /home/flexnetos/meta/flexnetos_runner/templates/git-upstream-worktree-sync/claude/commands/*.md .claude/commands/
cat /home/flexnetos/meta/flexnetos_runner/templates/git-upstream-worktree-sync/.gitignore.append >> .gitignore
chmod +x scripts/sync-upstream-worktree.sh
```

Optional per-repo secret/config copy list:

```bash
cp /home/flexnetos/meta/flexnetos_runner/templates/git-upstream-worktree-sync/.worktreeinclude.example .worktreeinclude
```

Edit `.worktreeinclude` before committing it. Do not list raw secrets in a
tracked file unless that repo already deliberately tracks those files.

## One-Off Sync

Make sure the fork has an upstream remote:

```bash
git remote -v
git remote add upstream https://github.com/OWNER/REPO.git
```

Create an isolated sync worktree and merge upstream's default branch:

```bash
scripts/sync-upstream-worktree.sh
```

Common explicit form:

```bash
scripts/sync-upstream-worktree.sh \
  --upstream upstream \
  --upstream-branch master \
  --base develop \
  --branch chore/sync-upstream-0.42.4 \
  --check-cmd 'cargo test --workspace'
```

The main checkout is left untouched. If the upstream merge conflicts, the
script stops inside the isolated worktree and prints the conflict status.

## Land Without A PR

Only do this when branch protection and team policy allow direct pushes:

```bash
git switch develop
git merge chore/sync-upstream-0.42.4
git push origin develop
```

If the branch is protected, push the sync branch and let the normal protected
branch path merge it:

```bash
git push -u origin chore/sync-upstream-0.42.4
```

## Claude Commands

After copying `claude/commands/*.md` into `.claude/commands/`:

```text
/sync-upstream
/sync-upstream --upstream upstream --upstream-branch master --base develop --branch chore/sync-upstream-0.42.4
/worktree chore/safe-local-change --fast
/worktree-status chore/safe-local-change
/clean-worktrees --dry-run
```

The Claude command layer is deliberately thin. The source of truth is the
repo-local `scripts/sync-upstream-worktree.sh` file copied from this template.

## Safety Rules

- Work happens in `.worktrees/<branch-slug>/`, not in the current checkout.
- `.worktrees/` is ignored by the repo or by local git exclude.
- The script fetches upstream before merging.
- The script never deletes branches, resets history, or force-pushes.
- Direct branch push is opt-in through `--push`.
- Any final merge into a protected branch is a human policy decision.
