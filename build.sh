#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

# ---------- helpers ----------
info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[OK]\033[0m    %s\n' "$*"; }
err()   { printf '\033[1;31m[ERR]\033[0m   %s\n' "$*" >&2; }

# ---------- CI targets ----------
# Each entry: "rust_target manylinux_arch"
# maturin handles the Python ABI tags automatically.
CI_TARGETS=(
    "x86_64-unknown-linux-gnu     x86_64"
    "aarch64-unknown-linux-gnu    aarch64"
    "x86_64-apple-darwin          -"
    "aarch64-apple-darwin         -"
    "x86_64-pc-windows-msvc       -"
)

# ---------- check prerequisites ----------
check_prereqs() {
    local missing=()
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

    if ! command -v maturin >/dev/null 2>&1; then
        info "Installing maturin..."
        pip install maturin
    fi
}

# ---------- parse args ----------
COMMAND=""
MODE="release"      # dev | release
RUN_TESTS=false

usage() {
    cat <<EOF
Usage: $0 [COMMAND] [OPTIONS]

Commands:
  (none)        Build only (default: release, current platform)
  install       Build and install
  uninstall     Remove installed package
  test          Run full test suite (build first if needed)
  ci            Build release wheels for all supported platforms

Options:
  --dev         Development/debug build
  --release     Optimised release build (default)
  --test        Run tests after build/install
  -h, --help    Show this help

Examples:
  $0                    Build release wheel (current platform)
  $0 --dev              Build debug wheel
  $0 install            Build release and install
  $0 install --dev      Editable development install
  $0 test               Build and run all tests
  $0 ci                 Build wheels for all platforms via Docker/cross
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        install)   COMMAND="install";   shift ;;
        uninstall) COMMAND="uninstall"; shift ;;
        test)      COMMAND="test";      shift ;;
        ci)        COMMAND="ci";        shift ;;
        --dev)     MODE="dev";          shift ;;
        --release) MODE="release";      shift ;;
        --test)    RUN_TESTS=true;      shift ;;
        -h|--help) usage ;;
        *) err "Unknown option: $1"; usage ;;
    esac
done

check_prereqs

# ---------- functions ----------
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

do_ci() {
    local dist_dir="$ROOT_DIR/dist"
    mkdir -p "$dist_dir"

    info "CI build: all platform wheels -> dist/"

    # ---- Linux targets via manylinux Docker ----
    if command -v docker >/dev/null 2>&1; then
        for entry in "${CI_TARGETS[@]}"; do
            read -r target arch <<< "$entry"
            case "$target" in
                *-linux-*)
                    info "Building $target (manylinux, arch=$arch)..."
                    docker run --rm \
                        -v "$ROOT_DIR":/io \
                        -w /io \
                        "ghcr.io/pyo3/maturin" \
                        build --release --find-interpreter --target "$target" -o /io/dist \
                        && ok "$target done" \
                        || err "$target failed (non-fatal)"
                    ;;
                *)
                    # macOS / Windows — only buildable on native or with cross
                    info "Skipping $target (not Linux, needs native runner or cross-compilation)"
                    ;;
            esac
        done
    else
        err "Docker not found — Linux manylinux builds require Docker."
        info "Falling back to local-platform-only build."
    fi

    # ---- Native platform build (always) ----
    info "Building native platform wheel..."
    maturin build --release -o "$dist_dir"
    ok "Native wheel built"

    # ---- Cross-compile non-Linux if toolchains are installed ----
    for entry in "${CI_TARGETS[@]}"; do
        read -r target arch <<< "$entry"
        case "$target" in
            *-linux-*) continue ;;  # already done via Docker
        esac

        if rustup target list --installed 2>/dev/null | grep -q "$target"; then
            info "Cross-compiling $target..."
            maturin build --release --target "$target" -o "$dist_dir" \
                && ok "$target done" \
                || err "$target failed (non-fatal)"
        fi
    done

    echo ""
    info "Wheels in dist/:"
    ls -1 "$dist_dir"/*.whl 2>/dev/null || info "(none)"

    local count
    count=$(ls -1 "$dist_dir"/*.whl 2>/dev/null | wc -l)
    ok "CI build complete: $count wheel(s) in dist/"
}

# ---------- dispatch ----------
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
    ci)
        do_ci
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
