#!/usr/bin/env bash
#
# Does cinrs compile SQLite?
#
#   scripts/check-sqlite.sh              both configurations, debug and release
#   scripts/check-sqlite.sh --quick      the default configuration in debug only
#   scripts/check-sqlite.sh --keep       do not delete the downloaded zip
#
# SQLite's amalgamation is the largest single C translation unit anyone ships:
# 269,649 lines of C in one file, a 7,000-line `switch` with `goto`s for the
# virtual machine, tables of function pointers, variadic definitions, bit-fields,
# `union` initialisers. Compiling it, linking it and running a query through it is
# the honest measure of whether this crate translates C rather than a subset of it,
# and it is the one check that needs the network.
#
# The 9 MB of C is not in this repository. It is downloaded from the pinned URL
# below and verified against the SHA3-256 that sqlite.org publishes on its
# download page; a mismatch is a failure and nothing is unpacked. The fixture
# crate is `tests/sqlite-fixture/`, outside the workspace, and is built twice:
# once with `SQLITE_THREADSAFE=1` (SQLite's own default, which on a Unix means
# pthreads) and once with `SQLITE_THREADSAFE=0`.
#
# Defining a variadic function needs **Rust 1.99** (`c_variadic`), and SQLite
# defines twenty of them — `sqlite3_mprintf`, `sqlite3_snprintf`, `sqlite3_log`,
# `sqlite3_config`, `sqlite3_str_appendf` and the rest. Below 1.99 the front end
# reports each as a located error rather than mistranslating it, so this script
# looks for a toolchain that is new enough and says which one it used.
# `CINRS_SQLITE_TOOLCHAIN` names one explicitly (`beta`, `nightly`, `1.99.0`, …).
#
# When the platform's own `libsqlite3` is on the machine, the identical query loop
# is run against it too, so that there is a number to compare. It is not a
# benchmark — one loop in one process, and the platform's library is a different
# release built with different options — but it is a first number.
#
# Like every other step of this project's verification it runs under a ceiling on
# the address space and one on the clock: `CINRS_SQLITE_ULIMIT_V` (kilobytes,
# 8000000 by default, `0` to switch it off) and `CINRS_SQLITE_TIMEOUT` (seconds
# per step).

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

# ---------------------------------------------------------------------------
# what is downloaded, and how it is checked
# ---------------------------------------------------------------------------
#
# Pinned on purpose: a check that follows "the latest release" is a check that
# changes under you. The hash is the one sqlite.org's download page publishes for
# this file, copied from there; the SHA-256 is recorded as well because
# `sha256sum` is on every machine and `sha3-256` needs an `openssl` new enough to
# have it.
SQLITE_VERSION=3.53.4
SQLITE_ZIP=sqlite-amalgamation-3530400.zip
SQLITE_URL=https://sqlite.org/2026/$SQLITE_ZIP
SQLITE_DIR_IN_ZIP=sqlite-amalgamation-3530400
# From https://sqlite.org/download.html, "C source code as an amalgamation,
# version 3.53.4":
SQLITE_SHA3_256=628a44cfe82c66aed1ccbbe85a562d2e33ebe64b3288981ed76285612227934e
# Computed from the file that hash identifies.
SQLITE_SHA256=1e71ddf93849c6a6ecf58b827c0692073d2dd7ee40196158068f7b29f422e87d

FIXTURE=$ROOT/tests/sqlite-fixture
WORK=$ROOT/target/sqlite
VMEM_KB=${CINRS_SQLITE_ULIMIT_V:-8000000}
SECONDS_LIMIT=${CINRS_SQLITE_TIMEOUT:-2400}
QUICK=0
KEEP=0

say() { printf '%s\n' "$*"; }
die() {
    say "check-sqlite: FAILED: $*"
    exit 1
}

for arg in "$@"; do
    case $arg in
    -q | --quick) QUICK=1 ;;
    -k | --keep) KEEP=1 ;;
    -h | --help)
        sed -n '3,38p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        printf 'check-sqlite: unknown argument %s (try --help)\n' "$arg" >&2
        exit 2
        ;;
    esac
done

# ---------------------------------------------------------------------------
# the toolchain
# ---------------------------------------------------------------------------

# The first of these that is installed and is 1.99 or newer, since a variadic
# *definition* needs `c_variadic`. An explicit `CINRS_SQLITE_TOOLCHAIN` is taken
# as given and is not second-guessed.
pick_toolchain() {
    if [ -n "${CINRS_SQLITE_TOOLCHAIN:-}" ]; then
        printf '%s\n' "$CINRS_SQLITE_TOOLCHAIN"
        return 0
    fi
    local candidate version major minor
    for candidate in "" beta nightly; do
        if [ -z "$candidate" ]; then
            version=$(cargo --version 2>/dev/null) || continue
        else
            version=$(cargo "+$candidate" --version 2>/dev/null) || continue
        fi
        # "cargo 1.99.0-beta.6 (…)" → 1 99
        version=${version#cargo }
        major=${version%%.*}
        minor=${version#*.}
        minor=${minor%%.*}
        case $major$minor in
        *[!0-9]*) continue ;;
        esac
        if [ "$major" -gt 1 ] || { [ "$major" -eq 1 ] && [ "$minor" -ge 99 ]; }; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

if ! TOOLCHAIN=$(pick_toolchain); then
    say "check-sqlite: skipped: no Rust 1.99 or newer toolchain is installed."
    say ""
    say "    SQLite *defines* twenty variadic functions — sqlite3_mprintf,"
    say "    sqlite3_snprintf, sqlite3_log and the rest — and defining one needs"
    say "    c_variadic, stable since 1.99. Below that the front end reports each"
    say "    as a located error rather than mistranslating it, so there is nothing"
    say "    this script could usefully run. Install one:"
    say ""
    say "        rustup toolchain install beta"
    say ""
    say "    or name one with CINRS_SQLITE_TOOLCHAIN."
    exit 0
fi
if [ -n "$TOOLCHAIN" ]; then
    CARGO=(cargo "+$TOOLCHAIN")
else
    CARGO=(cargo)
fi
say "check-sqlite: using $("${CARGO[@]}" --version)"

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

# SHA3-256 if this machine's openssl has it, SHA-256 otherwise. Both are recorded
# above, so a machine that can only compute one is still checking the file.
digest_matches() {
    local file=$1 got
    if got=$(openssl dgst -sha3-256 -r "$file" 2>/dev/null); then
        got=${got%% *}
        if [ "$got" = "$SQLITE_SHA3_256" ]; then
            say "check-sqlite: sha3-256 ok ($got)"
            return 0
        fi
        say "check-sqlite: sha3-256 mismatch"
        say "    expected $SQLITE_SHA3_256"
        say "    got      $got"
        return 1
    fi
    if got=$(sha256sum "$file" 2>/dev/null); then
        got=${got%% *}
        if [ "$got" = "$SQLITE_SHA256" ]; then
            say "check-sqlite: sha-256 ok ($got); openssl has no sha3-256 here"
            return 0
        fi
        say "check-sqlite: sha-256 mismatch"
        say "    expected $SQLITE_SHA256"
        say "    got      $got"
        return 1
    fi
    die "neither 'openssl dgst -sha3-256' nor 'sha256sum' works here, so the download cannot be verified"
}

AMALGAMATION=$WORK/sqlite3.c
ZIP=$WORK/$SQLITE_ZIP

if [ -f "$AMALGAMATION" ] && grep -q "define SQLITE_VERSION *\"$SQLITE_VERSION\"" "$WORK/sqlite3.h" 2>/dev/null; then
    say "check-sqlite: $AMALGAMATION is already SQLite $SQLITE_VERSION"
else
    say "check-sqlite: fetching $SQLITE_URL"
    fetch "$SQLITE_URL" "$ZIP" || die "could not download $SQLITE_URL"
    digest_matches "$ZIP" || die "the downloaded $SQLITE_ZIP is not the file sqlite.org published; nothing was unpacked"
    command -v unzip >/dev/null 2>&1 || die "unzip is not installed"
    rm -rf "$WORK/$SQLITE_DIR_IN_ZIP"
    unzip -q -o "$ZIP" -d "$WORK" || die "could not unpack $ZIP"
    for file in sqlite3.c sqlite3.h sqlite3ext.h; do
        mv -f "$WORK/$SQLITE_DIR_IN_ZIP/$file" "$WORK/$file"
    done
    rm -rf "$WORK/$SQLITE_DIR_IN_ZIP"
    [ "$KEEP" -eq 1 ] || rm -f "$ZIP"
    say "check-sqlite: unpacked $(wc -l <"$AMALGAMATION") lines of C into $WORK"
fi

grep -q "define SQLITE_VERSION *\"$SQLITE_VERSION\"" "$WORK/sqlite3.h" ||
    die "$WORK/sqlite3.h is not SQLite $SQLITE_VERSION"

# ---------------------------------------------------------------------------
# the platform's own libsqlite3, for comparison
# ---------------------------------------------------------------------------
#
# `-lsqlite3` needs an unversioned `libsqlite3.so`, which only the development
# package installs; a machine with just the runtime has `libsqlite3.so.0`. A
# symlink under `target/` is enough for the linker and costs nothing, so a
# comparison is available wherever the library is at all.
SYSTEM_FEATURE=()
SYSTEM_RUSTFLAGS=
find_system_sqlite() {
    local dir candidate
    for dir in /usr/lib/"$(uname -m)"-linux-gnu /usr/lib64 /usr/lib /usr/local/lib /opt/homebrew/lib /usr/lib/x86_64-linux-gnu; do
        [ -d "$dir" ] || continue
        for candidate in "$dir/libsqlite3.so" "$dir/libsqlite3.dylib" "$dir/libsqlite3.so.0"; do
            [ -e "$candidate" ] && printf '%s\n' "$candidate" && return 0
        done
    done
    return 1
}
if LIB=$(find_system_sqlite); then
    mkdir -p "$WORK/link"
    case $LIB in
    *.dylib) ln -sf "$LIB" "$WORK/link/libsqlite3.dylib" ;;
    *) ln -sf "$LIB" "$WORK/link/libsqlite3.so" ;;
    esac
    SYSTEM_FEATURE=(system-sqlite)
    SYSTEM_RUSTFLAGS="-L $WORK/link"
    say "check-sqlite: comparing against $LIB"
else
    say "check-sqlite: no platform libsqlite3 found; skipping the comparison"
fi

# ---------------------------------------------------------------------------
# build and run
# ---------------------------------------------------------------------------

FAILED=()

# One cargo invocation, under both ceilings, with its wall time and peak RSS
# printed. `/usr/bin/time` is not on every machine, so its absence only costs the
# numbers.
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
        export CARGO_TARGET_DIR=$ROOT/target/sqlite-fixture
        [ -n "$SYSTEM_RUSTFLAGS" ] && export RUSTFLAGS=$SYSTEM_RUSTFLAGS
        if command -v /usr/bin/time >/dev/null 2>&1; then
            exec /usr/bin/time -f "    $label: %es wall, %MKB peak RSS" \
                timeout -k 10 "$SECONDS_LIMIT" "$@"
        fi
        exec timeout -k 10 "$SECONDS_LIMIT" "$@"
    ) || status=$?
    if [ "$status" -ne 0 ]; then
        say "check-sqlite: FAILED: $label (exit $status)"
        FAILED+=("$label")
        return 1
    fi
    return 0
}

# The rlib the translation produced, as a number worth reporting.
report_size() {
    local profile=$1 rlib
    rlib=$(ls -S "$ROOT/target/sqlite-fixture/$profile/deps/"libcinrs_sqlite_fixture-*.rlib 2>/dev/null | head -1 || true)
    if [ -n "$rlib" ]; then
        say "    library: $(du -h "$rlib" | cut -f1) ($rlib)"
    fi
}

features() {
    local list=("$@")
    list+=("${SYSTEM_FEATURE[@]}")
    if [ ${#list[@]} -eq 0 ]; then
        return 0
    fi
    printf '%s' "$(
        IFS=,
        echo "${list[*]}"
    )"
}

DEFAULT_FEATURES=$(features)
NOTHREAD_FEATURES=$(features nothreads)

# SQLITE_THREADSAFE=1, debug: the one everything else is a variation on.
if [ -n "$DEFAULT_FEATURES" ]; then
    run_step "SQLITE_THREADSAFE=1, debug" "${CARGO[@]}" test --features "$DEFAULT_FEATURES" -- --nocapture --test-threads=1 || true
else
    run_step "SQLITE_THREADSAFE=1, debug" "${CARGO[@]}" test -- --nocapture --test-threads=1 || true
fi
report_size debug

if [ "$QUICK" -eq 0 ]; then
    if [ -n "$DEFAULT_FEATURES" ]; then
        run_step "SQLITE_THREADSAFE=1, release" "${CARGO[@]}" test --release --features "$DEFAULT_FEATURES" -- --nocapture --test-threads=1 || true
    else
        run_step "SQLITE_THREADSAFE=1, release" "${CARGO[@]}" test --release -- --nocapture --test-threads=1 || true
    fi
    report_size release

    run_step "SQLITE_THREADSAFE=0, debug" "${CARGO[@]}" test --features "$NOTHREAD_FEATURES" -- --nocapture --test-threads=1 || true
    run_step "SQLITE_THREADSAFE=0, release" "${CARGO[@]}" test --release --features "$NOTHREAD_FEATURES" -- --nocapture --test-threads=1 || true
fi

say ""
if [ ${#FAILED[@]} -eq 0 ]; then
    say "check-sqlite: ok: SQLite $SQLITE_VERSION compiles, links and runs"
    exit 0
fi
say "check-sqlite: FAILED: ${#FAILED[@]} step(s):"
for step in "${FAILED[@]}"; do
    say "  - $step"
done
exit 1
