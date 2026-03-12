#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
INSTALL_DIR="${ZEROCLAW_INSTALL_DIR:-$HOME/.cargo/bin}"
FORCE_ZEROCLAW_INSTALL=false
SKIP_ZEROCLAW_INSTALL=false
SOURCE_BUILD=false
SKIP_TESTS=false
SKIP_DOCTOR=false

info() {
  printf '==> %s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Bootstrap zeroclaw + rover-probe workspace

Usage:
  bash scripts/bootstrap.sh [options]

Options:
  --force-zeroclaw-install   Reinstall zeroclaw even if already present
  --skip-zeroclaw-install    Skip zeroclaw installation
  --source-build             Install zeroclaw from source instead of prebuilt binary
  --skip-tests               Skip `cargo test`
  --skip-doctor              Skip `cargo run -p rover-probe -- doctor`
  -h, --help                 Show help

Environment:
  ZEROCLAW_BIN               Explicit zeroclaw binary path for doctor
  ZEROCLAW_INSTALL_DIR       Install destination (default: ~/.cargo/bin)
EOF
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --force-zeroclaw-install)
        FORCE_ZEROCLAW_INSTALL=true
        ;;
      --skip-zeroclaw-install)
        SKIP_ZEROCLAW_INSTALL=true
        ;;
      --source-build)
        SOURCE_BUILD=true
        ;;
      --skip-tests)
        SKIP_TESTS=true
        ;;
      --skip-doctor)
        SKIP_DOCTOR=true
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown option: $1"
        ;;
    esac
    shift
  done
}

resolve_zeroclaw_bin() {
  if [[ -n "${ZEROCLAW_BIN:-}" ]]; then
    if [[ -x "${ZEROCLAW_BIN}" ]]; then
      printf '%s\n' "${ZEROCLAW_BIN}"
      return 0
    fi
    die "ZEROCLAW_BIN points to a missing or non-executable path: ${ZEROCLAW_BIN}"
  fi

  if have_cmd zeroclaw; then
    command -v zeroclaw
    return 0
  fi

  if [[ -x "${INSTALL_DIR}/zeroclaw" ]]; then
    printf '%s\n' "${INSTALL_DIR}/zeroclaw"
    return 0
  fi

  return 1
}

detect_release_target() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64|Darwin:aarch64)
      printf 'aarch64-apple-darwin\n'
      ;;
    Darwin:x86_64)
      printf 'x86_64-apple-darwin\n'
      ;;
    Linux:x86_64)
      printf 'x86_64-unknown-linux-gnu\n'
      ;;
    Linux:aarch64|Linux:arm64)
      printf 'aarch64-unknown-linux-gnu\n'
      ;;
    *)
      return 1
      ;;
  esac
}

install_zeroclaw_prebuilt() {
  local target archive_url temp_dir archive_path extracted_path

  have_cmd curl || die "curl is required for prebuilt zeroclaw install"
  have_cmd tar || die "tar is required for prebuilt zeroclaw install"

  target="$(detect_release_target)" || die "unsupported platform: $(uname -s) $(uname -m)"
  archive_url="https://github.com/zeroclaw-labs/zeroclaw/releases/latest/download/zeroclaw-${target}.tar.gz"
  temp_dir="$(mktemp -d)"
  archive_path="${temp_dir}/zeroclaw-${target}.tar.gz"

  info "downloading zeroclaw prebuilt asset for ${target}"
  curl -fsSL "${archive_url}" -o "${archive_path}"
  tar -xzf "${archive_path}" -C "${temp_dir}"

  if [[ -x "${temp_dir}/zeroclaw" ]]; then
    extracted_path="${temp_dir}/zeroclaw"
  else
    extracted_path="$(find "${temp_dir}" -maxdepth 3 -type f -name zeroclaw -perm -u+x | head -n 1 || true)"
  fi

  [[ -n "${extracted_path}" ]] || die "prebuilt archive did not contain zeroclaw"

  mkdir -p "${INSTALL_DIR}"
  install -m 0755 "${extracted_path}" "${INSTALL_DIR}/zeroclaw"
  export ZEROCLAW_BIN="${INSTALL_DIR}/zeroclaw"
  info "installed zeroclaw to ${ZEROCLAW_BIN}"
}

install_zeroclaw_from_source() {
  local temp_dir source_dir

  have_cmd git || die "git is required for source build"
  have_cmd cargo || die "cargo is required for source build"
  have_cmd rustc || die "rustc is required for source build"

  temp_dir="$(mktemp -d)"
  source_dir="${temp_dir}/zeroclaw"

  info "cloning zeroclaw source"
  git clone --depth 1 https://github.com/zeroclaw-labs/zeroclaw.git "${source_dir}"

  info "installing zeroclaw from source"
  (
    cd "${source_dir}"
    cargo install --path . --force --locked
  )

  export ZEROCLAW_BIN="${INSTALL_DIR}/zeroclaw"
  info "installed zeroclaw from source"
}

ensure_zeroclaw() {
  local existing_bin

  if "${SKIP_ZEROCLAW_INSTALL}"; then
    warn "skipping zeroclaw installation by request"
    return 0
  fi

  if existing_bin="$(resolve_zeroclaw_bin 2>/dev/null)" && [[ "${FORCE_ZEROCLAW_INSTALL}" != true ]]; then
    export ZEROCLAW_BIN="${existing_bin}"
    info "using existing zeroclaw at ${ZEROCLAW_BIN}"
    return 0
  fi

  if "${SOURCE_BUILD}"; then
    install_zeroclaw_from_source
  else
    install_zeroclaw_prebuilt
  fi
}

ensure_rust_workspace_prereqs() {
  have_cmd cargo || die "cargo is required to build this workspace"
  have_cmd rustc || die "rustc is required to build this workspace"
}

run_workspace_tests() {
  if "${SKIP_TESTS}"; then
    warn "skipping cargo test by request"
    return 0
  fi

  info "running cargo test"
  (
    cd "${REPO_ROOT}"
    cargo test
  )
}

run_doctor() {
  if "${SKIP_DOCTOR}"; then
    warn "skipping doctor by request"
    return 0
  fi

  info "running rover-probe doctor"
  (
    cd "${REPO_ROOT}"
    ZEROCLAW_BIN="${ZEROCLAW_BIN:-}" cargo run -p rover-probe -- doctor
  )
}

main() {
  parse_args "$@"

  ensure_rust_workspace_prereqs
  ensure_zeroclaw
  run_workspace_tests
  run_doctor

  info "bootstrap complete"
}

main "$@"
