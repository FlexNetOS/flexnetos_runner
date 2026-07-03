#!/usr/bin/env bash
# Build a local workspace release bundle for this workstation.
set -euo pipefail

ROOT="${FXRUN_WORKSPACE_ROOT:-${FLEXNETOS_ROOT:-/home/flexnetos/FlexNetOS}}"
OUT_ROOT="${FXRUN_RELEASE_DIR:-$ROOT/release}"
TARGET_OS_ID="ubuntu"
TARGET_OS_VERSION="26.04"
TARGET_ARCH="x86_64"
RELEASE_PREFIX="flexnetos-ubuntu-${TARGET_OS_VERSION}-${TARGET_ARCH}"
CATALOG="${FXRUN_RELEASE_CATALOG:-$ROOT/src/flexnetos_runner/release/catalog.tsv}"
COMPONENTS="${FXRUN_RELEASE_COMPONENTS:-}"
RUNNER_HOME="${FXRUN_RUNNER_HOME:-$ROOT/src/flexnetos_runner/_work/runner-home-01}"
TAURI_BUNDLES="${FXRUN_TAURI_BUNDLES:-deb}"
BUN_INSTALL_ROOT="${FXRUN_BUN_INSTALL:-$RUNNER_HOME/.bun}"
BUN_TMPDIR_ROOT="${FXRUN_BUN_TMPDIR:-$RUNNER_HOME/.cache/bun/tmp}"
CARGO_HOME_ROOT="${FXRUN_CARGO_HOME:-$RUNNER_HOME/.cargo}"
RUSTUP_HOME_ROOT="${FXRUN_RUSTUP_HOME:-$RUNNER_HOME/.rustup}"
KACHE_CACHE_ROOT="${FXRUN_KACHE_CACHE_DIR:-$RUNNER_HOME/.cache/kache}"
KACHE_CONFIG_PATH="${FXRUN_KACHE_CONFIG:-$RUNNER_HOME/.config/kache/config.toml}"
KACHE_RUSTC_WRAPPER="${FXRUN_KACHE_RUSTC_WRAPPER:-/home/flexnetos/FlexNetOS/usr/bin/kache-rustc-wrapper}"
KACHE_WRAPPER_SHIM="${FXRUN_KACHE_WRAPPER_SHIM:-$RUNNER_HOME/.cargo/bin/flexnetos-kache-rustc-wrapper}"

usage() {
  cat <<USAGE
Usage: $0 [--check-only] [--out DIR]

Build the local Ubuntu ${TARGET_OS_VERSION} ${TARGET_ARCH} workspace release bundle.

Environment:
  FXRUN_WORKSPACE_ROOT      Workspace root. Default: $ROOT
  FLEXNETOS_ROOT            Back-compat alias for FXRUN_WORKSPACE_ROOT
  FXRUN_RELEASE_DIR         Release output root. Default: \$FXRUN_WORKSPACE_ROOT/release
  FXRUN_RELEASE_CATALOG     Release component catalog. Default: $CATALOG
  FXRUN_RELEASE_COMPONENTS  Optional space-separated component filter. Default: all catalog rows
  FXRUN_CARGO               Cargo binary to use when cargo is not on PATH
  FXRUN_BUN                 Bun binary to use for JS/Tauri components
  FXRUN_RUNNER_HOME         Runner home used for Cargo/Bun writable state. Default: $RUNNER_HOME
  FXRUN_CARGO_HOME          Cargo home for native desktop builds. Default: $CARGO_HOME_ROOT
  FXRUN_RUSTUP_HOME         Rustup home for native desktop builds. Default: $RUSTUP_HOME_ROOT
  FXRUN_BUN_INSTALL         Bun install root for JS/Tauri builds. Default: $BUN_INSTALL_ROOT
  FXRUN_BUN_TMPDIR          Bun temp root for JS/Tauri builds. Default: $BUN_TMPDIR_ROOT
  FXRUN_KACHE_CACHE_DIR     Kache cache root for native builds. Default: $KACHE_CACHE_ROOT
  FXRUN_KACHE_CONFIG        Kache config file for native builds. Default: $KACHE_CONFIG_PATH
  FXRUN_KACHE_RUSTC_WRAPPER Cargo rustc-wrapper path for runner-local cargo config. Default: $KACHE_RUSTC_WRAPPER
  FXRUN_KACHE_WRAPPER_SHIM  Runner-local shim that pins Kache config/cache for Cargo wrapper mode. Default: $KACHE_WRAPPER_SHIM
  FXRUN_TAURI_BUNDLES       Comma-separated Tauri bundle list. Default: $TAURI_BUNDLES
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
  local runner_cargo="$RUNNER_HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo"
  if [[ -x "$runner_cargo" ]]; then
    echo "$runner_cargo"
    return 0
  fi
  command -v cargo || true
}

resolve_bun() {
  if [[ -n "${FXRUN_BUN:-}" ]]; then
    [[ -x "$FXRUN_BUN" ]] || fail "FXRUN_BUN is not executable: $FXRUN_BUN"
    echo "$FXRUN_BUN"
    return 0
  fi
  command -v bun || true
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

copy_tree_contents() {
  local source="$1" dest="$2"
  [[ -d "$source" ]] || fail "missing source tree: $source"
  mkdir -p "$dest"
  cp -a "$source/." "$dest/"
}

write_kache_config() {
  local config_path="$1"
  mkdir -p "$(dirname "$config_path")" "$KACHE_CACHE_ROOT"
  cat > "$config_path" <<EOF
[cache]
local_store = "$KACHE_CACHE_ROOT"
local_max_size = "50GiB"
local_only = true
clean_incremental = true
cache_executables = false
EOF
}

write_cargo_config() {
  local config_path="$1"
  mkdir -p "$(dirname "$config_path")"
  cat > "$config_path" <<EOF
[build]
rustc-wrapper = "$KACHE_WRAPPER_SHIM"
EOF
}

write_kache_wrapper_shim() {
  local shim_path="$1"
  mkdir -p "$(dirname "$shim_path")"
  cat > "$shim_path" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export KACHE_CONFIG="$KACHE_CONFIG_PATH"
export KACHE_CACHE_DIR="$KACHE_CACHE_ROOT"
exec "$KACHE_RUSTC_WRAPPER" "\$@"
EOF
  chmod 755 "$shim_path"
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

copy_lifeos_bundle_assets() {
  local lifeos_repo="$1" stage="$2"
  local bundle_root="$lifeos_repo/target/release/bundle"
  local dest="$stage/packages/lifeos"
  [[ -d "$bundle_root" ]] || fail "missing LifeOS bundle output: $bundle_root"
  copy_tree_contents "$bundle_root" "$dest"
}

stage_assets() {
  local profile="$1" source="$2" stage="$3"
  case "$profile" in
    -|"") ;;
    yazelix-runtime) copy_yazelix_runtime_assets "$source" "$stage" ;;
    lifeos-bundle) copy_lifeos_bundle_assets "$source" "$stage" ;;
    *) fail "unknown asset profile for catalog component: $profile" ;;
  esac
}

build_bun_tauri_component() {
  local name="$1" repo="$2" manifest="$3" bins="$4" asset_profile="$5" notes="$6" stage="$7" bun="$8" cargo="$9"
  local rust_bin_dir target_bin
  rust_bin_dir="$(dirname "$cargo")"
  target_bin="$repo/target/release/$bins"
  echo "==> building $name"
  mkdir -p "$BUN_INSTALL_ROOT" "$BUN_TMPDIR_ROOT" "$CARGO_HOME_ROOT" "$KACHE_CACHE_ROOT"
  write_kache_config "$KACHE_CONFIG_PATH"
  write_kache_wrapper_shim "$KACHE_WRAPPER_SHIM"
  write_cargo_config "$CARGO_HOME_ROOT/config.toml"
  (
    cd "$repo"
    env -i \
      HOME="$RUNNER_HOME" \
      PATH="$rust_bin_dir:/home/flexnetos/.local/bin:/usr/bin:/bin" \
      CARGO_HOME="$CARGO_HOME_ROOT" \
      CARGO_BUILD_RUSTC_WRAPPER="$KACHE_WRAPPER_SHIM" \
      RUSTUP_HOME="$RUSTUP_HOME_ROOT" \
      XDG_CONFIG_HOME="$RUNNER_HOME/.config" \
      XDG_CACHE_HOME="$RUNNER_HOME/.cache" \
      BUN_INSTALL="$BUN_INSTALL_ROOT" \
      BUN_TMPDIR="$BUN_TMPDIR_ROOT" \
      KACHE_CONFIG="$KACHE_CONFIG_PATH" \
      KACHE_CACHE_DIR="$KACHE_CACHE_ROOT" \
      "$bun" install --frozen-lockfile
    env -i \
      HOME="$RUNNER_HOME" \
      PATH="$rust_bin_dir:/home/flexnetos/.local/bin:/usr/bin:/bin" \
      CARGO_HOME="$CARGO_HOME_ROOT" \
      CARGO_BUILD_RUSTC_WRAPPER="$KACHE_WRAPPER_SHIM" \
      RUSTUP_HOME="$RUSTUP_HOME_ROOT" \
      XDG_CONFIG_HOME="$RUNNER_HOME/.config" \
      XDG_CACHE_HOME="$RUNNER_HOME/.cache" \
      BUN_INSTALL="$BUN_INSTALL_ROOT" \
      BUN_TMPDIR="$BUN_TMPDIR_ROOT" \
      KACHE_CONFIG="$KACHE_CONFIG_PATH" \
      KACHE_CACHE_DIR="$KACHE_CACHE_ROOT" \
      "$bun" x tauri build --bundles "$TAURI_BUNDLES"
  )
  mkdir -p "$stage/bin" "$stage/provenance/components/$name"
  [[ -x "$target_bin" ]] || fail "expected release binary is missing or not executable: $target_bin"
  cp "$target_bin" "$stage/bin/$bins"
  chmod 755 "$stage/bin/$bins"
  stage_assets "$asset_profile" "$repo" "$stage"
  {
    echo "name=$name"
    echo "kind=bun-tauri"
    echo "repo=$repo"
    echo "manifest=$manifest"
    echo "bins=$bins"
    echo "asset_profile=$asset_profile"
    echo "head=$(git_value "$repo" rev-parse HEAD)"
    echo "branch=$(git_value "$repo" branch --show-current)"
    echo "dirty=$(repo_dirty "$repo")"
    echo "bun=$bun"
    echo "cargo=$cargo"
    echo "tauri_bundles=$TAURI_BUNDLES"
    echo "bun_install=$BUN_INSTALL_ROOT"
    echo "bun_tmpdir=$BUN_TMPDIR_ROOT"
    echo "cargo_home=$CARGO_HOME_ROOT"
    echo "rustup_home=$RUSTUP_HOME_ROOT"
    echo "kache_wrapper_shim=$KACHE_WRAPPER_SHIM"
    echo "kache_rustc_wrapper=$KACHE_RUSTC_WRAPPER"
    echo "kache_config=$KACHE_CONFIG_PATH"
    echo "kache_cache_dir=$KACHE_CACHE_ROOT"
    echo "notes=$notes"
  } > "$stage/provenance/components/$name/source.env"
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
    bun-tauri)
      [[ -d "$source" ]] || fail "catalog source dir missing for $name: $source"
      [[ -f "$manifest" ]] || fail "catalog manifest missing for $name: $manifest"
      [[ "$bins" != "-" ]] || fail "bun-tauri component requires a staged binary name for $name"
      ;;
    *)
      fail "unknown catalog kind for $name: $kind"
      ;;
  esac
}

process_catalog() {
  local stage="$1" cargo="$2" bun="$3" mode="$4"
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
      bun-tauri) build_bun_tauri_component "$name" "$source" "$manifest" "$bins" "$asset_profile" "$notes" "$stage" "$bun" "$cargo" ;;
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
    echo "runner_home=$RUNNER_HOME"
    echo "tauri_bundles=$TAURI_BUNDLES"
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
  local bun cargo
  bun="$(resolve_bun)"
  [[ -n "$bun" ]] || fail "bun not found; set FXRUN_BUN to the LifeOS-compatible bun binary"
  cargo="$(resolve_cargo)"
  [[ -n "$cargo" ]] || fail "cargo not found; set FXRUN_CARGO to a runner-local cargo binary"
  export PATH="$(dirname "$cargo"):$PATH"
  mkdir -p "$BUN_INSTALL_ROOT" "$BUN_TMPDIR_ROOT" "$CARGO_HOME_ROOT" "$KACHE_CACHE_ROOT"
  write_kache_config "$KACHE_CONFIG_PATH"
  write_kache_wrapper_shim "$KACHE_WRAPPER_SHIM"
  write_cargo_config "$CARGO_HOME_ROOT/config.toml"
  "$cargo" --version >/dev/null
  "$bun" --version >/dev/null
  SELECTED_COMPONENTS=""

  if [[ "$CHECK_ONLY" == "1" ]]; then
    process_catalog "" "$cargo" "$bun" check
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

  process_catalog "$stage" "$cargo" "$bun" build

  write_provenance "$stage" "$cargo" "$release_id" "$(basename "$archive")"
  tar -C "$OUT_ROOT/staging" -czf "$archive" "$release_id"
  sha256sum "$archive" > "$archive.sha256"

  echo "release archive: $archive"
  echo "release sha256:  $archive.sha256"
  echo "stage:           $stage"
}

main "$@"
