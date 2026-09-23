#!/usr/bin/env bash
#
# Does cinrs compile xxHash?
#
#   scripts/check-xxhash.sh              debug and release
#   scripts/check-xxhash.sh --quick      debug only
#   scripts/check-xxhash.sh --keep       do not delete the downloaded archive
#
# xxHash is one header, `xxhash.h` (7,500 lines), that is the whole library when
# `XXH_IMPLEMENTATION` or `XXH_INLINE_ALL` is defined, plus `xxhash.c` to
# instantiate it and `xxh_x86dispatch.c` to pick a vector width at run time. The
# header chooses its XXH3 kernel with `XXH_VECTOR` — scalar, SSE2, AVX2 or
# AVX-512 on x86 — so one header compiled four ways is four programs, and the
# dispatcher compiles all of them into one unit behind
# `__attribute__((__target__))`, reading `cpuid` through inline assembly.
# `tests/sanity_test_vectors.h` holds upstream's generated table of expected
# XXH32, XXH64, XXH3-64 and XXH3-128 values, which says whether the answer is
# right.
#
# The C is not in this repository. The release archive is downloaded from the
# pinned URL below and verified against the pinned SHA-256; a mismatch is a
# failure and nothing is unpacked. Only the four library files, `LICENSE`,
# `tests/sanity_test.c` and `tests/sanity_test_vectors.h` are unpacked, into
# `target/xxhash/`. The fixture crate is `tests/xxhash-fixture/`, outside the
# workspace.
#
#   Upstream  https://github.com/Cyan4973/xxHash
#   Tag       v0.8.4
#   Archive   https://github.com/Cyan4973/xxHash/archive/refs/tags/v0.8.4.tar.gz
#   SHA-256   5738270935e7c3d38a79b3adf7c9692566ce7895a25f67de43ad52ab504acd32
#             (computed from the archive on 2026-09-24; GitHub publishes none)
#   Licence   BSD-2-Clause
#
# Like every other step of this project's verification it runs under a ceiling on
# the address space and one on the clock: `CINRS_XXHASH_ULIMIT_V` (kilobytes,
# 8000000 by default, `0` to switch it off) and `CINRS_XXHASH_TIMEOUT` (seconds
# per step). `CINRS_XXHASH_TOOLCHAIN` names a rustup toolchain (`beta`, …).

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

# ---------------------------------------------------------------------------
# what is downloaded, and how it is checked
# ---------------------------------------------------------------------------

XXHASH_TAG=v0.8.4
XXHASH_VERSION=${XXHASH_TAG#v}
XXHASH_TARBALL=xxHash-$XXHASH_TAG.tar.gz
XXHASH_URL=https://github.com/Cyan4973/xxHash/archive/refs/tags/$XXHASH_TAG.tar.gz
XXHASH_DIR_IN_TARBALL=xxHash-$XXHASH_VERSION
XXHASH_SHA256=5738270935e7c3d38a79b3adf7c9692566ce7895a25f67de43ad52ab504acd32

FIXTURE=$ROOT/tests/xxhash-fixture
WORK=$ROOT/target/xxhash
VMEM_KB=${CINRS_XXHASH_ULIMIT_V:-8000000}
SECONDS_LIMIT=${CINRS_XXHASH_TIMEOUT:-1200}
QUICK=0
KEEP=0

say() { printf '%s\n' "$*"; }
die() {
    say "check-xxhash: FAILED: $*"
    exit 1
}

for arg in "$@"; do
    case $arg in
    -q | --quick) QUICK=1 ;;
    -k | --keep) KEEP=1 ;;
    -h | --help)
        sed -n '3,36p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        printf 'check-xxhash: unknown argument %s (try --help)\n' "$arg" >&2
        exit 2
        ;;
    esac
done

if [ -n "${CINRS_XXHASH_TOOLCHAIN:-}" ]; then
    CARGO=(cargo "+$CINRS_XXHASH_TOOLCHAIN")
else
    CARGO=(cargo)
fi
say "check-xxhash: using $("${CARGO[@]}" --version)"

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
    if [ "$got" = "$XXHASH_SHA256" ]; then
        say "check-xxhash: sha-256 ok ($got)"
        return 0
    fi
    say "check-xxhash: sha-256 mismatch"
    say "    expected $XXHASH_SHA256"
    say "    got      $got"
    return 1
}

TARBALL=$WORK/$XXHASH_TARBALL

# `xxhash.h` spells its version as three macros, not a string.
is_this_version() {
    local h=$WORK/xxhash.h
    local major minor release
    IFS=. read -r major minor release <<<"$XXHASH_VERSION"
    [ -f "$h" ] &&
        grep -q "define XXH_VERSION_MAJOR *$major\$" "$h" &&
        grep -q "define XXH_VERSION_MINOR *$minor\$" "$h" &&
        grep -q "define XXH_VERSION_RELEASE *$release\$" "$h"
}

if is_this_version && [ -f "$WORK/xxhash.c" ] && [ -f "$WORK/xxh_x86dispatch.c" ] &&
    [ -f "$WORK/tests/sanity_test_vectors.h" ]; then
    say "check-xxhash: $WORK is already xxHash $XXHASH_TAG"
else
    if [ ! -f "$TARBALL" ]; then
        say "check-xxhash: fetching $XXHASH_URL"
        fetch "$XXHASH_URL" "$TARBALL" || die "could not download $XXHASH_URL"
    fi
    digest_matches "$TARBALL" || {
        rm -f "$TARBALL"
        die "the downloaded $XXHASH_TARBALL is not the pinned archive; nothing was unpacked"
    }
    rm -rf "$WORK/tests"
    rm -f "$WORK"/xxhash.h "$WORK"/xxhash.c "$WORK"/xxh_x86dispatch.[ch] "$WORK"/LICENSE
    tar -xzf "$TARBALL" -C "$WORK" --strip-components=1 \
        "$XXHASH_DIR_IN_TARBALL/xxhash.h" \
        "$XXHASH_DIR_IN_TARBALL/xxhash.c" \
        "$XXHASH_DIR_IN_TARBALL/xxh_x86dispatch.c" \
        "$XXHASH_DIR_IN_TARBALL/xxh_x86dispatch.h" \
        "$XXHASH_DIR_IN_TARBALL/LICENSE" \
        "$XXHASH_DIR_IN_TARBALL/tests/sanity_test.c" \
        "$XXHASH_DIR_IN_TARBALL/tests/sanity_test_vectors.h" ||
        die "could not unpack $TARBALL"
    [ "$KEEP" -eq 1 ] || rm -f "$TARBALL"
    say "check-xxhash: unpacked $(cat "$WORK"/xxhash.[ch] "$WORK"/xxh_x86dispatch.[ch] | wc -l) lines of C into $WORK"
fi

is_this_version || die "$WORK/xxhash.h is not xxHash $XXHASH_TAG"

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
        export CARGO_TARGET_DIR=$ROOT/target/xxhash-fixture
        if command -v /usr/bin/time >/dev/null 2>&1; then
            exec /usr/bin/time -f "    $label: %es wall, %MKB peak RSS" \
                timeout -k 10 "$SECONDS_LIMIT" "$@"
        fi
        exec timeout -k 10 "$SECONDS_LIMIT" "$@"
    ) || status=$?
    if [ "$status" -ne 0 ]; then
        say "check-xxhash: FAILED: $label (exit $status)"
        FAILED+=("$label")
        return 1
    fi
    return 0
}

# Five units: `xxhash.c` four times, once per `XXH_VECTOR`, each under its own
# `XXH_NAMESPACE`, and `xxh_x86dispatch.c`. The tests check each unit against
# upstream's sanity table, the units against each other, streaming against
# one-shot, and the dispatcher against all of them.
run_step "build and test, debug" "${CARGO[@]}" test -- --nocapture --test-threads=2 || true
if [ "$QUICK" -eq 0 ]; then
    run_step "build and test, release" "${CARGO[@]}" test --release -- --nocapture --test-threads=2 || true
fi
say ""
if [ ${#FAILED[@]} -eq 0 ]; then
    say "check-xxhash: ok: xxHash $XXHASH_TAG compiles, links and passes its sanity table"
    exit 0
fi
say "check-xxhash: FAILED: ${#FAILED[@]} step(s):"
for step in "${FAILED[@]}"; do
    say "  - $step"
done
exit 1
