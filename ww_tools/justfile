# The shipped tools (its own workspace): ww and the wire_weaver wheel, with logring's and chunky_mcu_updater's
# commands. Nothing in the repository root's recipes needs this workspace.

# List all available targets
default:
    @just --list

# The tests: ww's and the wheel's Rust tests, then the Python tests (`--changed` runs them all: this workspace is small)
test *flags:
    cargo nextest run --workspace --no-fail-fast
    just test-py

# Build the Python module (wire_weaver_wheel) into its own venv and run its tests, the conformance corpus too (TEST-23)
[working-directory('wire_weaver_wheel')]
test-py:
    uv run --group dev pytest

# The conformance corpus through the Python module only (TEST-23)
[working-directory('wire_weaver_wheel')]
test-conformance-py:
    uv run --group dev pytest -q tests/test_conformance.py

# Formatting, manifests, clippy and rustdoc of this workspace, warnings are errors
lint:
    cargo fmt --all --check
    cargo sort -w -g --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

# Python console with `ww` imported and the device connected as `dev` (rebuilds the module if needed), `just py --help`
[working-directory('wire_weaver_wheel')]
py *args:
    WW_CWD="{{invocation_directory()}}" uv run python -i scripts/console.py {{args}}

# Build ww and install it
install:
    cargo build --profile deploy --locked -p ww
    # With the internal release tool on PATH: install through it (kept-releases stack, one copy on PATH); otherwise: cargo install
    if command -v tpm >/dev/null; then tpm install target/deploy/ww; else cargo install --locked --path ww; fi

# Bring this PC up to main: pull --ff-only, refuse on local changes, install, check ww --version against main
deploy:
    #!/usr/bin/env bash
    set -eu
    dirty=$(git status --porcelain)
    if [ -n "$dirty" ]; then echo "local changes, not deploying:"; echo "$dirty"; exit 1; fi
    git pull --ff-only
    just install
    ww --version | grep -q "($(git rev-parse --short HEAD)" || { echo "ww --version does not show $(git rev-parse --short HEAD):"; ww --version; exit 1; }
    ww --version

# Build ww from a fresh clone of HEAD with nothing but this repository around (what `cargo install --git` sees)
check-clean-clone:
    rm -rf /tmp/ww-clean-clone && git clone -q "$(git rev-parse --show-toplevel)" /tmp/ww-clean-clone
    cd /tmp/ww-clean-clone/ww_tools && cargo build --locked -p ww
    rm -rf /tmp/ww-clean-clone
