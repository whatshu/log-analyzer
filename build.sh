#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

VERSION="0.1.0"

# ---------- helpers ----------
info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[OK]\033[0m    %s\n' "$*"; }
err()   { printf '\033[1;31m[ERR]\033[0m   %s\n' "$*" >&2; }

# ---------- CI targets ----------
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
MODE="release"
RUN_TESTS=false

usage() {
    cat <<EOF
Usage: $0 [COMMAND] [OPTIONS]

Commands:
  (none)        Build wheel only (default: release, current platform)
  install       Build and install
  uninstall     Remove installed package
  test          Run full test suite (build first if needed)
  pkg           Build deb + rpm packages for current platform
  ci            Build all wheels + deb + rpm

Options:
  --dev         Development/debug build
  --release     Optimised release build (default)
  --test        Run tests after build/install
  -h, --help    Show this help

Examples:
  $0                    Build release wheel (current platform)
  $0 install            Build release and install
  $0 install --dev      Editable development install
  $0 test               Build, install, and run all tests
  $0 pkg                Build deb + rpm for current platform
  $0 ci                 Build all platform wheels + deb + rpm
EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        install)   COMMAND="install";   shift ;;
        uninstall) COMMAND="uninstall"; shift ;;
        test)      COMMAND="test";      shift ;;
        pkg)       COMMAND="pkg";       shift ;;
        ci)        COMMAND="ci";        shift ;;
        --dev)     MODE="dev";          shift ;;
        --release) MODE="release";      shift ;;
        --test)    RUN_TESTS=true;      shift ;;
        -h|--help) usage ;;
        *) err "Unknown option: $1"; usage ;;
    esac
done

check_prereqs

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

# ---------- package (deb + rpm) ----------
ensure_nfpm() {
    if command -v nfpm >/dev/null 2>&1; then
        return 0
    fi

    info "nfpm not found. Installing..."
    local nfpm_ver="2.41.1"
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64)  arch="x86_64" ;;
        aarch64) arch="arm64"  ;;
        *) err "Unsupported architecture for nfpm: $arch"; return 1 ;;
    esac

    local url="https://github.com/goreleaser/nfpm/releases/download/v${nfpm_ver}/nfpm_${nfpm_ver}_linux_${arch}.tar.gz"
    local tmp_dir
    tmp_dir=$(mktemp -d)
    info "Downloading nfpm v${nfpm_ver}..."
    curl -fsSL "$url" | tar xz -C "$tmp_dir" nfpm

    # Install to project-local bin
    mkdir -p "$ROOT_DIR/.bin"
    mv "$tmp_dir/nfpm" "$ROOT_DIR/.bin/nfpm"
    rm -rf "$tmp_dir"
    export PATH="$ROOT_DIR/.bin:$PATH"
    ok "nfpm installed to .bin/nfpm"
}

do_pkg() {
    local dist_dir="$ROOT_DIR/dist"
    mkdir -p "$dist_dir"

    # Step 1: build the wheel
    do_build

    # Step 2: create a staging directory with a self-contained venv
    local staging
    staging=$(mktemp -d)
    local venv_dir="$staging/opt/log-analyzer"

    info "Creating self-contained virtualenv..."
    python3 -m venv "$venv_dir"
    "$venv_dir/bin/pip" install --quiet --upgrade pip
    "$venv_dir/bin/pip" install --quiet "$WHEEL"

    # Step 3: create CLI wrapper that uses the bundled venv
    mkdir -p "$staging/usr/bin"
    cat > "$staging/usr/bin/log-analyzer" <<'WRAPPER'
#!/bin/sh
exec /opt/log-analyzer/bin/python -m log_analyzer.cli "$@"
WRAPPER
    chmod +x "$staging/usr/bin/log-analyzer"

    # Step 4: generate nfpm config from template
    ensure_nfpm || { err "Cannot build packages without nfpm"; rm -rf "$staging"; return 1; }

    local arch
    arch=$(uname -m)

    local nfpm_cfg="$staging/nfpm.yaml"
    sed -e "s|__VERSION__|${VERSION}|g" \
        -e "s|__ARCH__|${arch}|g" \
        -e "s|__STAGING__|${staging}|g" \
        "$ROOT_DIR/nfpm.yaml" > "$nfpm_cfg"

    # Step 5: build deb
    info "Building deb..."
    nfpm pkg -f "$nfpm_cfg" -p deb -t "$dist_dir/" \
        && ok "deb built" \
        || err "deb build failed"

    # Step 6: build rpm
    info "Building rpm..."
    nfpm pkg -f "$nfpm_cfg" -p rpm -t "$dist_dir/" \
        && ok "rpm built" \
        || err "rpm build failed"

    rm -rf "$staging"

    info "Packages in dist/:"
    ls -1h "$dist_dir"/*.deb "$dist_dir"/*.rpm 2>/dev/null || info "(none)"
}

# ---------- ci ----------
do_ci() {
    local dist_dir="$ROOT_DIR/dist"
    mkdir -p "$dist_dir"

    info "CI build: all platform wheels + packages -> dist/"

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
                    info "Skipping $target (not Linux, needs native runner or cross-compilation)"
                    ;;
            esac
        done
    else
        err "Docker not found — Linux manylinux builds require Docker."
        info "Falling back to local-platform-only build."
    fi

    # ---- Native platform wheel (always) ----
    info "Building native platform wheel..."
    maturin build --release -o "$dist_dir"
    ok "Native wheel built"

    # ---- Cross-compile non-Linux if toolchains are installed ----
    for entry in "${CI_TARGETS[@]}"; do
        read -r target arch <<< "$entry"
        case "$target" in
            *-linux-*) continue ;;
        esac

        if rustup target list --installed 2>/dev/null | grep -q "$target"; then
            info "Cross-compiling $target..."
            maturin build --release --target "$target" -o "$dist_dir" \
                && ok "$target done" \
                || err "$target failed (non-fatal)"
        fi
    done

    # ---- System packages (deb + rpm) ----
    info "Building system packages..."
    do_pkg

    echo ""
    info "All artifacts in dist/:"
    ls -1h "$dist_dir"/ 2>/dev/null
    echo ""
    local whl_count deb_count rpm_count
    whl_count=$(ls -1 "$dist_dir"/*.whl 2>/dev/null | wc -l)
    deb_count=$(ls -1 "$dist_dir"/*.deb 2>/dev/null | wc -l)
    rpm_count=$(ls -1 "$dist_dir"/*.rpm 2>/dev/null | wc -l)
    ok "CI complete: ${whl_count} wheel(s), ${deb_count} deb(s), ${rpm_count} rpm(s)"
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
    pkg)
        do_pkg
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
