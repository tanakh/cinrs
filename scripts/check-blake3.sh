#!/usr/bin/env bash
#
# Does cinrs compile BLAKE3's C implementation?
#
#   scripts/check-blake3.sh              debug and release
#   scripts/check-blake3.sh --quick      debug only
#   scripts/check-blake3.sh --keep       do not delete the downloaded archive
#
# BLAKE3's C implementation is small — about 3,000 lines in seven files — but it
# is the SIMD side of cinrs in one program: SSE2, SSE4.1, AVX2 and AVX-512
# intrinsics in four files that upstream compiles with four different `-m`
# flags, a dispatcher that reads `cpuid` and `xgetbv` with inline assembly and
# picks one at run time, and official test vectors that say whether the answer
# is right.
#
# The C is not in this repository. The release archive is downloaded from the
# pinned URL below and verified against the pinned SHA-256; a mismatch is a
# failure and nothing is unpacked. Only `c/`, `test_vectors/test_vectors.json`
# and the licence texts are unpacked, into `target/blake3/`. The fixture crate is
# `tests/blake3-fixture/`, outside the workspace.
#
#   Upstream  https://github.com/BLAKE3-team/BLAKE3
#   Tag       1.8.7
#   Archive   https://github.com/BLAKE3-team/BLAKE3/archive/refs/tags/1.8.7.tar.gz
#   SHA-256   c6782a28842b1c0478524ac06a4f2ede784038ee298d6e2162c0b089c4306a3c
#             (computed from the archive on 2026-09-24; GitHub publishes none)
#   Licence   CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception
#
# Like every other step of this project's verification it runs under a ceiling on
# the address space and one on the clock: `CINRS_BLAKE3_ULIMIT_V` (kilobytes,
# 8000000 by default, `0` to switch it off) and `CINRS_BLAKE3_TIMEOUT` (seconds
# per step). `CINRS_BLAKE3_TOOLCHAIN` names a rustup toolchain (`beta`, …).

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

# ---------------------------------------------------------------------------
# what is downloaded, and how it is checked
# ---------------------------------------------------------------------------

BLAKE3_TAG=1.8.7
BLAKE3_TARBALL=BLAKE3-$BLAKE3_TAG.tar.gz
BLAKE3_URL=https://github.com/BLAKE3-team/BLAKE3/archive/refs/tags/$BLAKE3_TAG.tar.gz
BLAKE3_DIR_IN_TARBALL=BLAKE3-$BLAKE3_TAG
BLAKE3_SHA256=c6782a28842b1c0478524ac06a4f2ede784038ee298d6e2162c0b089c4306a3c

FIXTURE=$ROOT/tests/blake3-fixture
WORK=$ROOT/target/blake3
VMEM_KB=${CINRS_BLAKE3_ULIMIT_V:-8000000}
SECONDS_LIMIT=${CINRS_BLAKE3_TIMEOUT:-1200}
QUICK=0
KEEP=0

say() { printf '%s\n' "$*"; }
die() {
    say "check-blake3: FAILED: $*"
    exit 1
}

for arg in "$@"; do
    case $arg in
    -q | --quick) QUICK=1 ;;
    -k | --keep) KEEP=1 ;;
    -h | --help)
        sed -n '3,31p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        printf 'check-blake3: unknown argument %s (try --help)\n' "$arg" >&2
        exit 2
        ;;
    esac
done

if [ -n "${CINRS_BLAKE3_TOOLCHAIN:-}" ]; then
    CARGO=(cargo "+$CINRS_BLAKE3_TOOLCHAIN")
else
    CARGO=(cargo)
fi
say "check-blake3: using $("${CARGO[@]}" --version)"

# ---------------------------------------------------------------------------
# fetch and verify
# ---------------------------------------------------------------------------

mkdir -p "$WORK"

fetch() {
    local url=$1 out=$2
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$out" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$out" "$url"
    else
        die "neither curl nor wget is installed; cannot fetch $url"
    fi
}

digest_matches() {
    local file=$1 got
    got=$(sha256sum "$file" 2>/dev/null) ||
        die "'sha256sum' does not work here, so the download cannot be verified"
    got=${got%% *}
    if [ "$got" = "$BLAKE3_SHA256" ]; then
        say "check-blake3: sha-256 ok ($got)"
        return 0
    fi
    say "check-blake3: sha-256 mismatch"
    say "    expected $BLAKE3_SHA256"
    say "    got      $got"
    return 1
}

TARBALL=$WORK/$BLAKE3_TARBALL

if [ -f "$WORK/c/blake3.h" ] && grep -q "define BLAKE3_VERSION_STRING \"$BLAKE3_TAG\"" "$WORK/c/blake3.h" &&
    [ -f "$WORK/test_vectors/test_vectors.json" ]; then
    say "check-blake3: $WORK/c is already BLAKE3 $BLAKE3_TAG"
else
    if [ ! -f "$TARBALL" ]; then
        say "check-blake3: fetching $BLAKE3_URL"
        fetch "$BLAKE3_URL" "$TARBALL" || die "could not download $BLAKE3_URL"
    fi
    digest_matches "$TARBALL" || {
        rm -f "$TARBALL"
        die "the downloaded $BLAKE3_TARBALL is not the pinned archive; nothing was unpacked"
    }
    rm -rf "$WORK/c" "$WORK/test_vectors"
    tar -xzf "$TARBALL" -C "$WORK" --strip-components=1 \
        "$BLAKE3_DIR_IN_TARBALL/c" \
        "$BLAKE3_DIR_IN_TARBALL/test_vectors/test_vectors.json" \
        "$BLAKE3_DIR_IN_TARBALL/LICENSE_CC0" \
        "$BLAKE3_DIR_IN_TARBALL/LICENSE_A2" ||
        die "could not unpack $TARBALL"
    [ "$KEEP" -eq 1 ] || rm -f "$TARBALL"
    say "check-blake3: unpacked $(cat "$WORK"/c/blake3*.c | wc -l) lines of C into $WORK/c"
fi

grep -q "define BLAKE3_VERSION_STRING \"$BLAKE3_TAG\"" "$WORK/c/blake3.h" ||
    die "$WORK/c/blake3.h is not BLAKE3 $BLAKE3_TAG"

# ---------------------------------------------------------------------------
# build and run
# ---------------------------------------------------------------------------

FAILED=()

run_step() {
    local label=$1
    shift
    say ""
    say "=== $label ==="
    local status=0
    (
        if [ "$VMEM_KB" != 0 ]; then
            limit=$VMEM_KB
            hard=$(ulimit -H -v 2>/dev/null || echo unlimited)
            case $hard in
            '' | unlimited) ;;
            *) if [ "$hard" -lt "$limit" ] 2>/dev/null; then limit=$hard; fi ;;
            esac
            ulimit -v "$limit" 2>/dev/null || true
        fi
        cd "$FIXTURE"
        export CARGO_TARGET_DIR=$ROOT/target/blake3-fixture
        if command -v /usr/bin/time >/dev/null 2>&1; then
            exec /usr/bin/time -f "    $label: %es wall, %MKB peak RSS" \
                timeout -k 10 "$SECONDS_LIMIT" "$@"
        fi
        exec timeout -k 10 "$SECONDS_LIMIT" "$@"
    ) || status=$?
    if [ "$status" -ne 0 ]; then
        say "check-blake3: FAILED: $label (exit $status)"
        FAILED+=("$label")
        return 1
    fi
    return 0
}

# The build upstream ships: seven units, the dispatcher choosing at run time.
# The tests run the official vectors through the dispatcher, and each SIMD
# implementation this machine has against the portable one.
run_step "full build, debug" "${CARGO[@]}" test -- --nocapture --test-threads=2 || true
if [ "$QUICK" -eq 0 ]; then
    run_step "full build, release" "${CARGO[@]}" test --release -- --nocapture --test-threads=2 || true
fi

say ""
if [ ${#FAILED[@]} -eq 0 ]; then
    say "check-blake3: ok: BLAKE3 $BLAKE3_TAG compiles, links and passes its test vectors"
    exit 0
fi
say "check-blake3: FAILED: ${#FAILED[@]} step(s):"
for step in "${FAILED[@]}"; do
    say "  - $step"
done
exit 1
