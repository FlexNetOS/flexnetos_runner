{
  # FlexNetOS self-hosted GitHub runner — composed design, pure Nix flake.
  #
  # TWO LAYERS, ONE CLOSURE (zero OS system dependencies — no systemd, no host
  # services, no apt/host packages):
  #
  #   1. SUBSTRATE — nixpkgs `github-runner` (the real actions/runner). Registered
  #      to the FlexNetOS org with labels [self-hosted, flexnetos, nix]; executes
  #      ALL GitHub workflows/actions, including the archbp-* environment tests.
  #      nixpkgs patches it to resolve mutable state from env vars, so it runs
  #      from the immutable store with state under the profile-runtime link.
  #
  #   2. rUv AGENT LAYER — a metaharness harness (@metaharness/kernel +
  #      @metaharness/host-github-actions + agentic-flow) that workflows invoke
  #      AS A STEP on the substrate (ADR-033; cf. worldgraph.yml with
  #      runs-on swapped to the self-hosted labels). Plain ESM bin/cli.js, no
  #      build step, executed via bun.
  #
  # Grounded in rUv source:
  #   metaharness/docs/adrs/ADR-033-host-github-actions.md   (GHA host adapter, --gha-mode)
  #   metaharness/packages/host-github-actions/package.json  (v0.1.2)
  #   worldgraph/.github/workflows/worldgraph.yml            (harness runs ON a runner)
  description = "FlexNetOS self-hosted GitHub runner: github-runner substrate + rUv metaharness agent layer (hermetic Nix, no OS deps)";

  inputs = {
    # Pinned to a recent nixos-unstable rev whose github-runner + dotnet closure is
    # prebuilt in cache.nixos.org (substitutes in seconds; no dotnet-vmr source build).
    # Reproducible (explicit rev) without dragging in an aged, uncached toolchain.
    nixpkgs.url = "github:NixOS/nixpkgs/241313f4e8e508cb9b13278c2b0fa25b9ca27163";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system});
      runnerScript = ./scripts/runner.nu;
      mkRunnerLaunch = system: pkgs:
        let
          nu = "${pkgs.nushell}/bin/nu";
        in
        pkgs.writeShellScript "flexnetos-runner" ''
          export GHA_SUBSTRATE=${pkgs.github-runner}
          export GHA_BUN=${pkgs.bun}/bin/bun
          export GHA_HARNESS=${self.packages.${system}.metaharness}/lib/node_modules/flexnetos-runner/bin/cli.js
          exec ${nu} ${runnerScript} "$@"
        '';
      mkRunnerStart = system: pkgs:
        let
          nu = "${pkgs.nushell}/bin/nu";
          launch = mkRunnerLaunch system pkgs;
          start = pkgs.writeShellScriptBin "flexnetos-runner-start" ''
            export PATH="$HOME/.nix-profile/toolbin:$HOME/.nix-profile/bin:${pkgs.coreutils}/bin"
            export GHA_NU=${nu}
            export GHA_MINT_SCRIPT=${./scripts/mint-runner-token.nu}
            export GHA_RUNNER_LAUNCH=${launch}
            exec ${nu} ${./scripts/runner-start.nu} "$@"
          '';
        in
        # NO_SYSTEM_DEPTHS: this is a foreground, per-session Nix-store
        # entrypoint. Unattended reboot activation is deliberately unsupported
        # because it requires an external supervisor.
        start;
    in
    {
      packages = forAll (system: pkgs: {
        # Layer 1: the real actions/runner from nixpkgs — executes all workflows.
        substrate = pkgs.github-runner;

        # Layer 2: the metaharness agent harness, built hermetically from npm.
        metaharness = pkgs.buildNpmPackage {
          pname = "metaharness";
          version = "0.4.1";
          src = ./harness;
          # dontNpmBuild: the harness bin/cli.js is plain ESM — no tsc build step.
          dontNpmBuild = true;
          # Computed with prefetch-npm-deps over harness/package-lock.json.
          npmDepsHash = "sha256-oOFpGJYI8NSSinLyAhLCghuadNla3v39vgYU5YDucso=";
          # Hermetic: no postinstall network (sharp/libvips); kernel runs on its wasm backend.
          npmFlags = [ "--ignore-scripts" ];
          nativeBuildInputs = [ pkgs.nodejs ];
          meta.description = "FlexNetOS metaharness runner harness (github-actions host)";
        };

        # Profile entrypoint for an explicit foreground session. It does not
        # depend on a source checkout and does not install an activation unit.
        runner-start = mkRunnerStart system pkgs;

        default = self.packages.${system}.substrate;
      });

      # Apps — all launched via nix, PATH/env from the closure only, Nushell entry.
      apps = forAll (system: pkgs:
        let
          launch = mkRunnerLaunch system pkgs;
        in
        {
          # doctor | register | run | agent — see scripts/runner.nu
          runner = {
            type = "app";
            program = toString launch;
          };
          # Explicit foreground session (mint → register --replace → run).
          # The Nix store is passive, so this intentionally promises no reboot
          # auto-start.
          start = {
            type = "app";
            program = "${self.packages.${system}.runner-start}/bin/flexnetos-runner-start";
          };
          # One-time (network) scaffold of the harness targeting the github-actions host.
          # Runtime uses bun (bunx) per FlexNetOS convention — never bare npx/node.
          scaffold = {
            type = "app";
            program = toString (pkgs.writeShellScript "mh-scaffold" ''
              exec ${pkgs.bun}/bin/bunx metaharness@0.4.1 scaffold \
                --name flexnetos-runner --hosts github-actions "$@"
            '');
          };
          # Offline gate harness (bun run, never bare node).
          verify = {
            type = "app";
            program = toString (pkgs.writeShellScript "mh-verify" ''
              exec ${pkgs.bun}/bin/bun run ${./verify.mjs} "$@"
            '');
          };
          default = self.apps.${system}.runner;
        });

      # Hermetic dev shell: bun + node (buildNpmPackage only) + nushell + gh, nothing from the host OS.
      devShells = forAll (system: pkgs: {
        default = pkgs.mkShell {
          packages = [ pkgs.bun pkgs.nodejs pkgs.nushell pkgs.gh pkgs.github-runner ];
          shellHook = ''
            echo "FlexNetOS gha-runner devshell — bun $(bun --version), nu $(nu --version | head -1)"
          '';
        };
      });
    };
}
