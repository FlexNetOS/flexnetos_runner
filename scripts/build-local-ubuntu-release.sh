#!/usr/bin/env bash
# Build a local FlexNetOS release bundle for this workstation.
set -euo pipefail

ROOT="${FLEXNETOS_ROOT:-/home/flexnetos/FlexNetOS}"
OUT_ROOT="${FXRUN_RELEASE_DIR:-$ROOT/release}"
TARGET_OS_ID="ubuntu"
TARGET_OS_VERSION="26.04"
TARGET_ARCH="x86_64"
RELEASE_PREFIX="flexnetos-ubuntu-${TARGET_OS_VERSION}-${TARGET_ARCH}"
COMPONENTS="${FXRUN_RELEASE_COMPONENTS:-flexnetos_runner meta yazelix}"

usage() {
  cat <<USAGE
Usage: $0 [--check-only] [--out DIR]

Build the local Ubuntu ${TARGET_OS_VERSION} ${TARGET_ARCH} FlexNetOS release bundle.

Environment:
  FLEXNETOS_ROOT            Workspace root. Default: $ROOT
  FXRUN_RELEASE_DIR         Release output root. Default: \$FLEXNETOS_ROOT/release
  FXRUN_RELEASE_COMPONENTS  Space-separated component list. Default: "$COMPONENTS"
  FXRUN_CARGO               Cargo binary to use when cargo is not on PATH
  FXRUN_RELEASE_ALLOW_HOST_MISMATCH=1
                            Allow running checks on a non-target host

Default components:
  flexnetos_runner  src/flexnetos_runner
  meta              src/meta
  yazelix           src/yazelix/rust_core
USAGE
}

CHECK_ONLY=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --check-only) CHECK_ONLY=1; shift ;;
    --out) OUT_ROOT="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

fail() {
  echo "error: $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

host_check() {
  [[ "$(uname -s)" == "Linux" ]] || fail "release target is Linux only"
  [[ "$(uname -m)" == "$TARGET_ARCH" ]] || fail "release target is $TARGET_ARCH only"
  [[ -r /etc/os-release ]] || fail "cannot read /etc/os-release"

  # shellcheck disable=SC1091
  . /etc/os-release
  if [[ "${ID:-}" != "$TARGET_OS_ID" || "${VERSION_ID:-}" != "$TARGET_OS_VERSION" ]]; then
    if [[ "${FXRUN_RELEASE_ALLOW_HOST_MISMATCH:-0}" == "1" ]]; then
      echo "warning: host is ${ID:-unknown} ${VERSION_ID:-unknown}; target is $TARGET_OS_ID $TARGET_OS_VERSION" >&2
    else
      fail "host is ${ID:-unknown} ${VERSION_ID:-unknown}; target is $TARGET_OS_ID $TARGET_OS_VERSION"
    fi
  fi
}

resolve_cargo() {
  if [[ -n "${FXRUN_CARGO:-}" ]]; then
    [[ -x "$FXRUN_CARGO" ]] || fail "FXRUN_CARGO is not executable: $FXRUN_CARGO"
    echo "$FXRUN_CARGO"
    return 0
  fi
  command -v cargo || true
}

git_value() {
  local repo="$1" key="$2"
  git -C "$repo" "$key" 2>/dev/null || true
}

repo_dirty() {
  local repo="$1"
  if [[ -n "$(git -C "$repo" status --porcelain 2>/dev/null)" ]]; then
    echo "true"
  else
    echo "false"
  fi
}

copy_release_bins() {
  local target_dir="$1" dest="$2"
  local copied=0
  [[ -d "$target_dir" ]] || fail "missing target dir after build: $target_dir"
  while IFS= read -r -d '' bin; do
    cp "$bin" "$dest/"
    copied=$((copied + 1))
  done < <(find "$target_dir" -maxdepth 1 -type f -executable ! -name '*.d' -print0)
  [[ "$copied" -gt 0 ]] || fail "no executable release binaries found in $target_dir"
}

build_component() {
  local name="$1" repo="$2" manifest="$3" stage="$4" cargo="$5"
  local manifest_dir target_dir
  manifest_dir="$(cd "$(dirname "$manifest")" && pwd)"
  target_dir="$manifest_dir/target/release"
  echo "==> building $name"
  "$cargo" build --release --manifest-path "$manifest" --locked
  mkdir -p "$stage/bin" "$stage/provenance/components/$name"
  copy_release_bins "$target_dir" "$stage/bin"
  {
    echo "name=$name"
    echo "repo=$repo"
    echo "manifest=$manifest"
    echo "head=$(git_value "$repo" rev-parse HEAD)"
    echo "branch=$(git_value "$repo" branch --show-current)"
    echo "dirty=$(repo_dirty "$repo")"
  } > "$stage/provenance/components/$name/source.env"
}

copy_yazelix_runtime_assets() {
  local yazelix_repo="$1" stage="$2"
  local dest="$stage/share/yazelix"
  mkdir -p "$dest"
  for path in \
    assets \
    configs \
    shells \
    zellij_config \
    yazi \
    yazelix_nushell_config.nu \
    README.md
  do
    if [[ -e "$yazelix_repo/$path" ]]; then
      cp -a "$yazelix_repo/$path" "$dest/"
    fi
  done
  rm -f "$dest/shells/posix/yazelix_runtime_size_report.sh"
  rm -f "$dest/shells/posix/yzx_cli.sh"
}

write_provenance() {
  local stage="$1" cargo="$2" release_id="$3" archive_name="$4"
  local manifest="$stage/provenance/release-manifest.env"
  {
    echo "release_id=$release_id"
    echo "target_os=$TARGET_OS_ID"
    echo "target_os_version=$TARGET_OS_VERSION"
    echo "target_arch=$TARGET_ARCH"
    echo "generated_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host=$(hostname)"
    echo "root=$ROOT"
    echo "components=$COMPONENTS"
    echo "cargo=$cargo"
    "$cargo" --version | sed 's/^/cargo_version=/'
    "$cargo" rustc --version 2>/dev/null | sed 's/^/rustc_version=/' || true
    echo "archive=$archive_name"
  } > "$manifest"

  find "$stage/bin" -maxdepth 1 -type f -executable -print0 \
    | sort -z \
    | xargs -0 sha256sum > "$stage/provenance/binary-sha256s.txt"
}

main() {
  need date
  need find
  need git
  need hostname
  need sha256sum
  need tar

  host_check
  local cargo
  cargo="$(resolve_cargo)"
  [[ -n "$cargo" ]] || fail "cargo not found; set FXRUN_CARGO to a runner-local cargo binary"
  export PATH="$(dirname "$cargo"):$PATH"
  "$cargo" --version >/dev/null

  if [[ "$CHECK_ONLY" == "1" ]]; then
    echo "release checks passed"
    echo "cargo=$cargo"
    echo "components=$COMPONENTS"
    exit 0
  fi

  local stamp release_id stage archive
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  release_id="${RELEASE_PREFIX}-${stamp}"
  stage="$OUT_ROOT/staging/$release_id"
  archive="$OUT_ROOT/$release_id.tar.gz"

  rm -rf "$stage"
  mkdir -p "$stage/bin" "$stage/provenance" "$OUT_ROOT"

  local component
  for component in $COMPONENTS; do
    case "$component" in
      flexnetos_runner)
        build_component "$component" "$ROOT/src/flexnetos_runner" "$ROOT/src/flexnetos_runner/Cargo.toml" "$stage" "$cargo"
        ;;
      meta)
        build_component "$component" "$ROOT/src/meta" "$ROOT/src/meta/Cargo.toml" "$stage" "$cargo"
        ;;
      yazelix)
        build_component "$component" "$ROOT/src/yazelix" "$ROOT/src/yazelix/rust_core/Cargo.toml" "$stage" "$cargo"
        copy_yazelix_runtime_assets "$ROOT/src/yazelix" "$stage"
        ;;
      *)
        fail "unknown release component: $component"
        ;;
    esac
  done

  write_provenance "$stage" "$cargo" "$release_id" "$(basename "$archive")"
  tar -C "$OUT_ROOT/staging" -czf "$archive" "$release_id"
  sha256sum "$archive" > "$archive.sha256"

  echo "release archive: $archive"
  echo "release sha256:  $archive.sha256"
  echo "stage:           $stage"
}

main "$@"
