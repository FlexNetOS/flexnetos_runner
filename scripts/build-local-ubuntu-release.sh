#!/usr/bin/env bash
# Build a local FlexNetOS release bundle for this workstation.
set -euo pipefail

ROOT="${FLEXNETOS_ROOT:-/home/flexnetos/FlexNetOS}"
OUT_ROOT="${FXRUN_RELEASE_DIR:-$ROOT/release}"
TARGET_OS_ID="ubuntu"
TARGET_OS_VERSION="26.04"
TARGET_ARCH="x86_64"
RELEASE_PREFIX="flexnetos-ubuntu-${TARGET_OS_VERSION}-${TARGET_ARCH}"
CATALOG="${FXRUN_RELEASE_CATALOG:-$ROOT/src/flexnetos_runner/release/catalog.tsv}"
COMPONENTS="${FXRUN_RELEASE_COMPONENTS:-}"

usage() {
  cat <<USAGE
Usage: $0 [--check-only] [--out DIR]

Build the local Ubuntu ${TARGET_OS_VERSION} ${TARGET_ARCH} FlexNetOS release bundle.

Environment:
  FLEXNETOS_ROOT            Workspace root. Default: $ROOT
  FXRUN_RELEASE_DIR         Release output root. Default: \$FLEXNETOS_ROOT/release
  FXRUN_RELEASE_CATALOG     Release component catalog. Default: $CATALOG
  FXRUN_RELEASE_COMPONENTS  Optional space-separated component filter. Default: all catalog rows
  FXRUN_CARGO               Cargo binary to use when cargo is not on PATH
  FXRUN_RELEASE_ALLOW_HOST_MISMATCH=1
                            Allow running checks on a non-target host

Catalog format:
  component<TAB>kind<TAB>source<TAB>manifest<TAB>bins<TAB>asset_profile<TAB>notes
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

resolve_path() {
  local path="$1"
  if [[ "$path" == "-" || "$path" == /* ]]; then
    echo "$path"
  else
    echo "$ROOT/$path"
  fi
}

selected_component() {
  local name="$1" component
  [[ -z "$COMPONENTS" ]] && return 0
  for component in $COMPONENTS; do
    [[ "$component" == "$name" ]] && return 0
  done
  return 1
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

copy_named_bins() {
  local target_dir="$1" dest="$2" bins="$3"
  local copied=0
  [[ -d "$target_dir" ]] || fail "missing target dir after build: $target_dir"
  [[ "$bins" == "-" ]] && return 0

  local old_ifs="$IFS" bin src
  IFS=,
  for bin in $bins; do
    src="$target_dir/$bin"
    [[ -x "$src" ]] || fail "expected release binary is missing or not executable: $src"
    cp "$src" "$dest/"
    copied=$((copied + 1))
  done
  IFS="$old_ifs"
  [[ "$copied" -gt 0 ]] || fail "no executable release binaries found in $target_dir"
}

build_component() {
  local name="$1" repo="$2" manifest="$3" bins="$4" asset_profile="$5" notes="$6" stage="$7" cargo="$8"
  local manifest_dir target_dir
  manifest_dir="$(cd "$(dirname "$manifest")" && pwd)"
  target_dir="$manifest_dir/target/release"
  echo "==> building $name"
  "$cargo" build --release --manifest-path "$manifest" --locked
  mkdir -p "$stage/bin" "$stage/provenance/components/$name"
  copy_named_bins "$target_dir" "$stage/bin" "$bins"
  stage_assets "$asset_profile" "$repo" "$stage"
  {
    echo "name=$name"
    echo "kind=cargo"
    echo "repo=$repo"
    echo "manifest=$manifest"
    echo "bins=$bins"
    echo "asset_profile=$asset_profile"
    echo "head=$(git_value "$repo" rev-parse HEAD)"
    echo "branch=$(git_value "$repo" branch --show-current)"
    echo "dirty=$(repo_dirty "$repo")"
    echo "notes=$notes"
  } > "$stage/provenance/components/$name/source.env"
}

copy_bin_component() {
  local name="$1" source="$2" bins="$3" asset_profile="$4" notes="$5" stage="$6"
  [[ "$bins" != "-" ]] || fail "copy-bin component requires staged binary name: $name"
  [[ "$bins" != *,* ]] || fail "copy-bin component supports one binary per row: $name"
  [[ -x "$source" ]] || fail "copy-bin source is missing or not executable: $source"
  echo "==> staging $name"
  mkdir -p "$stage/bin" "$stage/provenance/components/$name"
  cp "$source" "$stage/bin/$bins"
  chmod 755 "$stage/bin/$bins"
  stage_assets "$asset_profile" "$(dirname "$source")" "$stage"
  {
    echo "name=$name"
    echo "kind=copy-bin"
    echo "source=$source"
    echo "bins=$bins"
    echo "asset_profile=$asset_profile"
    echo "notes=$notes"
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

stage_assets() {
  local profile="$1" source="$2" stage="$3"
  case "$profile" in
    -|"") ;;
    yazelix-runtime) copy_yazelix_runtime_assets "$source" "$stage" ;;
    *) fail "unknown asset profile for catalog component: $profile" ;;
  esac
}

validate_catalog_row() {
  local name="$1" kind="$2" source="$3" manifest="$4" bins="$5"
  [[ -n "$name" && -n "$kind" && -n "$source" && -n "$manifest" && -n "$bins" ]] || fail "invalid catalog row for component: ${name:-<empty>}"
  case "$kind" in
    cargo)
      [[ -d "$source" ]] || fail "catalog source dir missing for $name: $source"
      [[ -f "$manifest" ]] || fail "catalog manifest missing for $name: $manifest"
      ;;
    copy-bin)
      [[ -x "$source" ]] || fail "catalog executable missing for $name: $source"
      [[ "$manifest" == "-" ]] || fail "copy-bin manifest must be '-' for $name"
      ;;
    *)
      fail "unknown catalog kind for $name: $kind"
      ;;
  esac
}

process_catalog() {
  local stage="$1" cargo="$2" mode="$3"
  [[ -f "$CATALOG" ]] || fail "release catalog not found: $CATALOG"

  local name kind source manifest bins asset_profile notes extra
  local selected=""
  while IFS=$'\t' read -r name kind source manifest bins asset_profile notes extra || [[ -n "${name:-}" ]]; do
    [[ -z "${name:-}" || "${name:0:1}" == "#" ]] && continue
    [[ -z "${extra:-}" ]] || fail "catalog row has too many fields: $name"
    selected_component "$name" || continue

    source="$(resolve_path "$source")"
    manifest="$(resolve_path "$manifest")"
    validate_catalog_row "$name" "$kind" "$source" "$manifest" "$bins"
    selected="${selected}${selected:+ }$name"

    if [[ "$mode" == "check" ]]; then
      echo "catalog ok: $name ($kind)"
      continue
    fi

    case "$kind" in
      cargo) build_component "$name" "$source" "$manifest" "$bins" "$asset_profile" "$notes" "$stage" "$cargo" ;;
      copy-bin) copy_bin_component "$name" "$source" "$bins" "$asset_profile" "$notes" "$stage" ;;
    esac
  done < "$CATALOG"

  [[ -n "$selected" ]] || fail "no catalog components selected"
  SELECTED_COMPONENTS="$selected"
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
    echo "catalog=$CATALOG"
    echo "components=$SELECTED_COMPONENTS"
    echo "cargo=$cargo"
    "$cargo" --version | sed 's/^/cargo_version=/'
    "$cargo" rustc --version 2>/dev/null | sed 's/^/rustc_version=/' || true
    echo "archive=$archive_name"
  } > "$manifest"
  cp "$CATALOG" "$stage/provenance/catalog.tsv"

  local sha_file="$stage/provenance/binary-sha256s.txt"
  : > "$sha_file"
  while IFS= read -r -d '' bin; do
    sha256sum "$bin" >> "$sha_file"
  done < <(find "$stage/bin" -maxdepth 1 -type f -executable -print0 | sort -z)
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
  SELECTED_COMPONENTS=""

  if [[ "$CHECK_ONLY" == "1" ]]; then
    process_catalog "" "$cargo" check
    echo "release checks passed"
    echo "cargo=$cargo"
    echo "catalog=$CATALOG"
    echo "components=$SELECTED_COMPONENTS"
    exit 0
  fi

  local stamp release_id stage archive
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  release_id="${RELEASE_PREFIX}-${stamp}"
  stage="$OUT_ROOT/staging/$release_id"
  archive="$OUT_ROOT/$release_id.tar.gz"

  rm -rf "$stage"
  mkdir -p "$stage/bin" "$stage/provenance" "$OUT_ROOT"

  process_catalog "$stage" "$cargo" build

  write_provenance "$stage" "$cargo" "$release_id" "$(basename "$archive")"
  tar -C "$OUT_ROOT/staging" -czf "$archive" "$release_id"
  sha256sum "$archive" > "$archive.sha256"

  echo "release archive: $archive"
  echo "release sha256:  $archive.sha256"
  echo "stage:           $stage"
}

main "$@"
