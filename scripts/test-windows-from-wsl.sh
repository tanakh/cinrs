#!/usr/bin/env bash
#
# Build, link and *run* the Windows half of the `portability` CI job from WSL2.
#
#   scripts/test-windows-from-wsl.sh              the examples and every test file
#   scripts/test-windows-from-wsl.sh execute vla  those test files only
#   scripts/test-windows-from-wsl.sh --list       print the test files and stop
#   scripts/test-windows-from-wsl.sh --help
#
# What it needs on the Windows side: Visual Studio (any edition from 2017 on)
# with the C++ toolset, and a Windows SDK — the same two things a `cl.exe` build
# needs. On the Linux side: `lld-link` (Ubuntu's `lld` package) and
# `rustup target add x86_64-pc-windows-msvc`. The `.exe` files are run through
# WSL's interop, which is on unless `/etc/wsl.conf` turns it off.
#
# What it does: finds the newest MSVC toolset and Windows SDK under
# /mnt/c/Program Files*, symlinks their x64 library directories into
# `target/windows-from-wsl/libs` so that no `-Lnative` argument has a space in
# it, points Cargo at `lld-link`, and runs `cargo test --target
# x86_64-pc-windows-msvc` one test file at a time.
#
# `CINRS_TARGET` is set explicitly. The macro runs on the *host*, so on this
# machine it would otherwise translate the C for LP64 and every `sizeof` in the
# expansion would be the Linux answer; on a real Windows machine the host model
# is already the right one and `build.rs` settles it. One file at a time because
# a compile error in one test target stops the others, and because a crashed
# Windows process shows up only as an exit status (5, the top byte of
# 0xC0000005 lost) — with one file per run it is clear which.
#
# The test files are `scripts/portability-tests.txt`, which
# `.github/workflows/ci.yml` reads for the same job: this script and that job
# run the same list by construction.
#
# Every step runs under `ulimit -v` and `timeout(1)`, as everything in this
# repository does. The *Windows* processes are outside the address-space limit —
# the NT kernel has never heard of it — so what keeps them small is that these
# tests are small. `CINRS_CI_ULIMIT_V` (kilobytes, 8000000 by default; `0`
# switches it off) overrides the limit, `CINRS_WIN_TIMEOUT` the per-file clock.

set -uo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

TARGET=x86_64-pc-windows-msvc
WORK=$ROOT/target/windows-from-wsl
LIBS=$WORK/libs
VMEM_KB=${CINRS_CI_ULIMIT_V:-8000000}
SECS=${CINRS_WIN_TIMEOUT:-1500}
LIST=scripts/portability-tests.txt

say() { printf '%s\n' "$*"; }

die() {
    printf 'test-windows-from-wsl: %s\n' "$*" >&2
    exit 2
}

usage() {
    sed -n '3,38p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# The test files this job runs, in the order the list gives them.
read_list() {
    [ -f "$LIST" ] || die "$LIST is missing"
    while read -r name; do
        case "$name" in '' | '#'*) continue ;; esac
        printf '%s\n' "$name"
    done <"$LIST"
}

# newest <glob>... — the last path a sorted glob expansion gives, which for
# version-numbered directories is the highest version. Nothing if none match.
newest() {
    local found=() path
    for path in "$@"; do
        [ -e "$path" ] && found+=("$path")
    done
    [ "${#found[@]}" -gt 0 ] || return 1
    # `sort -V` so that 14.44.35207 comes after 14.9.x.
    printf '%s\n' "${found[@]}" | sort -V | tail -1
}

WANTED=()
for arg in "$@"; do
    case $arg in
    -h | --help)
        usage
        exit 0
        ;;
    --list)
        read_list
        exit 0
        ;;
    -*) die "unknown option $arg (try --help)" ;;
    *) WANTED+=("$arg") ;;
    esac
done

# --- the two things this machine has to have --------------------------------

command -v lld-link >/dev/null 2>&1 || die \
    "lld-link is not on PATH. It is the linker: on Ubuntu, 'sudo apt install lld'."

if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    die "the $TARGET target is not installed: 'rustup target add $TARGET'."
fi

# --- the MSVC toolset and the Windows SDK, from the Windows side ------------

MSVC=$(newest /mnt/c/Program\ Files*/Microsoft\ Visual\ Studio/*/*/VC/Tools/MSVC/*/lib/x64) || die \
    "no MSVC toolset found under /mnt/c/Program Files*/Microsoft Visual Studio/*/*/VC/Tools/MSVC/*/lib/x64.
 Install Visual Studio's 'Desktop development with C++' workload on the Windows
 side, and check that the C: drive is mounted (ls /mnt/c)."

UCRT=$(newest /mnt/c/Program\ Files\ \(x86\)/Windows\ Kits/10/Lib/*/ucrt/x64) || die \
    "no Windows SDK found under '/mnt/c/Program Files (x86)/Windows Kits/10/Lib/*/ucrt/x64'.
 Install a Windows 10 or 11 SDK with Visual Studio."

UM=${UCRT%/ucrt/x64}/um/x64
[ -d "$UM" ] || die "the SDK at ${UCRT%/ucrt/x64} has no um/x64 directory"

# The symlinks are what keep the spaces in those paths out of the linker's
# argument list: `lld-link` is handed one `-Lnative` per directory and neither
# Cargo's RUSTFLAGS nor `rustc`'s own splitting quotes a space.
rm -rf "$LIBS"
mkdir -p "$LIBS"
ln -s "$MSVC" "$LIBS/msvc"
ln -s "$UCRT" "$LIBS/ucrt"
ln -s "$UM" "$LIBS/um"

say "test-windows-from-wsl: $(cargo --version), $(rustc --version)"
say "test-windows-from-wsl: MSVC  $MSVC"
say "test-windows-from-wsl: UCRT  $UCRT"
say "test-windows-from-wsl: um    $UM"
say "test-windows-from-wsl: ${VMEM_KB} kB and ${SECS}s per step"

export CINRS_TARGET=$TARGET
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Lnative=$LIBS/msvc -Lnative=$LIBS/ucrt -Lnative=$LIBS/um"

FAILED=()

# run <label> -- <command>...
run() {
    local label=$1
    shift
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
        exec timeout -k 10 "$SECS" "$@"
    ) || {
        local code=$?
        say "test-windows-from-wsl: FAILED ($code): $label"
        # 5 is 0xC0000005 with its top byte lost on the way through `wait`:
        # the Windows process faulted rather than failing an assertion.
        [ "$code" -eq 5 ] && say \
            "    exit 5 is an access violation. Run the .exe under
    target/windows-from-wsl/$TARGET/debug/deps/ with --test-threads=1 to see which test."
        FAILED+=("$label")
        return 0
    }
    say "test-windows-from-wsl: ok: $label"
}

# The examples are what the README shows, and they call `printf` — which on an
# MSVC target is the one thing cinrs links a library of its own for.
for example in readme fact; do
    run "example $example" \
        cargo run -q --locked --target "$TARGET" --target-dir "$WORK" --example "$example"
done

if [ "${#WANTED[@]}" -gt 0 ]; then
    FILES=("${WANTED[@]}")
else
    mapfile -t FILES < <(read_list)
fi

for name in "${FILES[@]}"; do
    run "test $name" \
        cargo test -q --locked --target "$TARGET" --target-dir "$WORK" \
        -p cinrs --test "$name" -- --test-threads=2
done

say ""
say "test-windows-from-wsl: ${#FILES[@]} test file(s) and 2 examples"
if [ "${#FAILED[@]}" -eq 0 ]; then
    say "test-windows-from-wsl: everything passed"
    exit 0
fi
say "test-windows-from-wsl: ${#FAILED[@]} step(s) failed:"
for label in "${FAILED[@]}"; do say "  - $label"; done
exit 1
