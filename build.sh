#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

# ---------- helpers ----------
info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[OK]\033[0m    %s\n' "$*"; }
err()   { printf '\033[1;31m[ERR]\033[0m   %s\n' "$*" >&2; }

# ---------- check prerequisites ----------
info "Checking prerequisites..."

missing=()
command -v python3 >/dev/null 2>&1 || missing+=(python3)
command -v cargo   >/dev/null 2>&1 || missing+=(cargo)
command -v rustc   >/dev/null 2>&1 || missing+=(rustc)

if (( ${#missing[@]} )); then
    err "Missing: ${missing[*]}"
    err "Install Python >= 3.10 and Rust toolchain (https://rustup.rs) first."
    exit 1
fi

PYTHON_VER=$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')
RUST_VER=$(rustc --version | awk '{print $2}')
info "Python $PYTHON_VER  |  Rust $RUST_VER"

# ---------- install maturin if needed ----------
if ! command -v maturin >/dev/null 2>&1; then
    info "Installing maturin..."
    pip install maturin
fi

# ---------- parse args ----------
COMMAND=""
MODE="release"      # dev | release
RUN_TESTS=false

usage() {
    cat <<EOF
Usage: $0 [COMMAND] [OPTIONS]

Commands:
  (none)        Build only (default: release)
  install       Build and install
  uninstall     Remove installed package
  test          Run full test suite (build first if needed)

Options:
  --dev         Development/debug build
  --release     Optimised release build (default)
  --test        Run tests after build/install
  -h, --help    Show this help

Examples:
  $0                    Build release wheel
  $0 --dev              Build debug wheel
  $0 install            Build release and install
  $0 install --dev      Editable development install
  $0 test               Build and run all tests
  $0 --test             Build release wheel, then run tests
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        install)   COMMAND="install";   shift ;;
        uninstall) COMMAND="uninstall"; shift ;;
        test)      COMMAND="test";      shift ;;
        --dev)     MODE="dev";        shift ;;
        --release) MODE="release";    shift ;;
        --test)    RUN_TESTS=true;    shift ;;
        -h|--help) usage ;;
        *) err "Unknown option: $1"; usage ;;
    esac
done

# ---------- build ----------
do_build() {
    if [[ "$MODE" == "dev" ]]; then
        info "Building debug..."
        maturin build
    else
        info "Building release..."
        maturin build --release
    fi
    WHEEL=$(ls -t target/wheels/*.whl | head -1)
    ok "Wheel built: $WHEEL"
}

do_install() {
    if [[ "$MODE" == "dev" ]]; then
        info "Installing in editable/development mode..."
        pip install -e ".[dev]"
        ok "Installed (editable).  CLI: log-analyzer --help"
    else
        do_build
        info "Installing $WHEEL ..."
        pip install --force-reinstall "$WHEEL"
        ok "Installed from $WHEEL"
    fi
}

do_test() {
    info "Running Rust tests..."
    cargo test
    ok "Rust tests passed"

    info "Running Python tests..."
    python3 -m pytest tests/ -v
    ok "Python tests passed"
}

case "${COMMAND:-}" in
    install)
        do_install
        ;;
    uninstall)
        info "Uninstalling log-analyzer..."
        pip uninstall -y log-analyzer 2>/dev/null && ok "Uninstalled." || ok "Not installed."
        ;;
    test)
        do_install
        do_test
        ;;
    *)
        do_build
        ;;
esac

if $RUN_TESTS; then
    do_test
fi

echo ""
ok "Done."
