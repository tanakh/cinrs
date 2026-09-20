#!/usr/bin/env bash
#
# The verification recipe, as one command: formatting, lints, the tests, the
# `--no-default-features` build, and the documentation.
#
#   scripts/ci.sh            fmt, clippy, the workspace tests, no-default-features, docs
#   scripts/ci.sh --full     the above, then the rust-analyzer check and the three
#                            conformance harnesses, each skipped with a note when
#                            what it needs — a binary, a corpus — is absent
#
# Every step runs under the two ceilings this project does not run anything
# without — `ulimit -v` on the address space and `timeout(1)` on the clock. A
# conformance run is a thousand `rustc` processes and a thousand programs a
# front end under development wrote, and either half can ask for all of memory;
# the harnesses have their own four ceilings (see doc/testsuites.md, "The safety
# rules"), and these are the outer pair that do not depend on the harness being
# the thing that went wrong. `CINRS_CI_ULIMIT_V` (kilobytes, 8000000 by
# default; `0` switches the outer limit off) overrides it.
#
# The two big corpora are *not* fetched here: the harnesses that need them skip
# themselves, successfully and with a note, when they are missing — which is
# what makes this script the same script in CI and on a laptop. Run
# `scripts/fetch-testsuites.sh` first and add `--full` to run them for real.

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

VMEM_KB=${CINRS_CI_ULIMIT_V:-8000000}
FULL=0
FAILED=()

say() { printf '%s\n' "$*"; }

die() {
    printf 'ci: %s\n' "$*" >&2
    exit 2
}

# step <seconds> <label> -- <command>...
#
# Runs one command under `ulimit -v` and `timeout`, in a subshell so that the
# limit is gone again afterwards. The limit is clamped to the inherited hard
# limit, because a session that has already lowered it would otherwise turn
# every step into an error about `ulimit` rather than a test result.
step() {
    local secs=$1 label=$2
    shift 2
    say ""
    say "=== $label ==="
    (
        if [ "$VMEM_KB" != 0 ]; then
            local hard limit=$VMEM_KB
            hard=$(ulimit -H -v 2>/dev/null || echo unlimited)
            case $hard in
            '' | unlimited) ;;
            *) if [ "$hard" -lt "$limit" ] 2>/dev/null; then limit=$hard; fi ;;
            esac
            ulimit -v "$limit" 2>/dev/null || true
        fi
        exec timeout -k 10 "$secs" "$@"
    ) || {
        local code=$?
        say "ci: FAILED ($code): $label"
        FAILED+=("$label")
        return 0
    }
    say "ci: ok: $label"
}

# corpus <directory> — is the corpus there?
corpus() { [ -d "$1" ]; }

for arg in "$@"; do
    case $arg in
    --full) FULL=1 ;;
    -h | --help)
        sed -n '3,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *) die "unknown argument $arg (try --help)" ;;
    esac
done

say "ci: $(cargo --version), $(rustc --version)"
say "ci: address-space limit ${VMEM_KB} kB per step"
say ""
say "ci: conformance corpora (the harnesses skip themselves without them):"
scripts/fetch-testsuites.sh --check || true

step 300 "cargo fmt --all --check" \
    cargo fmt --all --check

step 1800 "cargo clippy --workspace --all-targets -D warnings" \
    cargo clippy --workspace --all-targets --locked -- -D warnings

# The two big harnesses are neutralised with a filter that matches nothing,
# which is the only way to skip a corpus that *is* present; `--full` below runs
# them properly. `c_testsuite` is a submodule and cheap, so it runs here.
step 2400 "cargo test --workspace" \
    env CINRS_GCC_TORTURE_FILTER=__none__ CINRS_CLANG_C_FILTER=__none__ \
    cargo test -q --workspace --locked -- --test-threads=2

step 1200 "cargo test -p cinrs --no-default-features" \
    cargo test -q -p cinrs --no-default-features --locked -- --test-threads=2

step 600 "cargo doc --no-deps" \
    env RUSTDOCFLAGS="-D warnings" \
    cargo doc --no-deps --locked -p cinrs -p cinrs-core -p cinrs-macros -p cinrs-rt

if [ "$FULL" -eq 1 ]; then
    # What an editor makes of a crate that uses cinrs, which is a question only
    # rust-analyzer itself can answer: it hands a procedural macro tokens with no
    # positions at all, and a `c99!` block's text has to be recovered anyway. The
    # script skips itself, successfully and with a note, where there is no
    # rust-analyzer binary at all, and brings its own ceilings, so it is not
    # wrapped in `step`. (`.github/workflows/ci.yml` runs it as a step of its
    # own, with the rustup component, rather than through `--full`: that would
    # turn the conformance harnesses on too.)
    say ""
    say "=== scripts/check-rust-analyzer.sh ==="
    if scripts/check-rust-analyzer.sh; then
        say "ci: ok: scripts/check-rust-analyzer.sh"
    else
        say "ci: FAILED: scripts/check-rust-analyzer.sh"
        FAILED+=("scripts/check-rust-analyzer.sh")
    fi

    # Each harness in its default *guard* mode: every case not in the
    # expected-failure list must pass, and a listed case that has started
    # passing is reported. `…_REQUIRED=1` turns the harness's own skip into a
    # failure, which is what we want once we know the corpus is there.
    if corpus third_party/c-testsuite/tests/single-exec; then
        step 1800 "cargo test --test c_testsuite (guard)" \
            env CINRS_CTESTSUITE_REQUIRED=1 \
            cargo test -q -p cinrs --test c_testsuite --locked -- --test-threads=2
    else
        say ""
        say "ci: skipped c_testsuite: third_party/c-testsuite is not checked out"
        say "    (git submodule update --init third_party/c-testsuite)"
    fi

    if corpus third_party/gcc/gcc/testsuite/gcc.c-torture/execute; then
        step 3600 "cargo test --test gcc_torture (guard)" \
            env CINRS_TESTSUITES_REQUIRED=1 \
            cargo test -q -p cinrs --test gcc_torture --locked -- --test-threads=2
    else
        say ""
        say "ci: skipped gcc_torture: third_party/gcc is not present"
        say "    (scripts/fetch-testsuites.sh gcc)"
    fi

    if corpus third_party/llvm-project/clang/test/C; then
        step 1800 "cargo test --test clang_c (guard)" \
            env CINRS_TESTSUITES_REQUIRED=1 \
            cargo test -q -p cinrs --test clang_c --locked -- --test-threads=2
    else
        say ""
        say "ci: skipped clang_c: third_party/llvm-project is not present"
        say "    (scripts/fetch-testsuites.sh llvm)"
    fi
fi

say ""
if [ "${#FAILED[@]}" -eq 0 ]; then
    say "ci: everything passed"
    exit 0
fi
say "ci: ${#FAILED[@]} step(s) failed:"
for label in "${FAILED[@]}"; do say "  - $label"; done
exit 1
