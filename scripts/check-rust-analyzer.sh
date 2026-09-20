#!/usr/bin/env bash
#
# Does rust-analyzer see anything wrong with a crate that uses cinrs?
#
#   scripts/check-rust-analyzer.sh            analyse the fixture crate
#   scripts/check-rust-analyzer.sh --verbose   … and print everything it said
#
# This is the one check that cannot be written as a Rust test, because what it
# tests is the *host*: rust-analyzer hands a procedural macro tokens with no
# positions at all — no file, no source text, line 1 column 0 for every token
# alike — and a `c99!` block's text has to be recovered anyway. The unit tests in
# `crates/cinrs-core` reproduce those conditions exactly; this runs the real
# thing over `tests/rust-analyzer-fixture`, which uses every input form there is:
# raw tokens with `#include <…>`, `#include "…"` of a header beside the file,
# `#define`, `#if`, a string-literal block, `include_c99!`, and one block written
# twice word for word. Anything at all reported about the fixture's own files is a
# failure.
#
# It needs a rust-analyzer binary, and looks for one in this order:
#
#   $CINRS_RUST_ANALYZER            an explicit path
#   rust-analyzer                   on $PATH, if it actually runs — the rustup
#                                   proxy of that name errors with "Unknown
#                                   binary" unless the component is installed
#   ~/.vscode-server/extensions/rust-lang.rust-analyzer-*/server/rust-analyzer
#   ~/.vscode/extensions/rust-lang.rust-analyzer-*/server/rust-analyzer
#
# the newest of the extension copies, and skips with a note — successfully, like
# the conformance harnesses without their corpora — when there is none. That is
# what lets `scripts/ci.sh --full` run it unconditionally.
#
# Like every other step of this project's verification it runs under a ceiling on
# the address space and one on the clock: `CINRS_RA_ULIMIT_V` (kilobytes, 8000000
# by default, `0` to switch it off) and `CINRS_RA_TIMEOUT` (seconds).

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

FIXTURE=$ROOT/tests/rust-analyzer-fixture
VMEM_KB=${CINRS_RA_ULIMIT_V:-8000000}
SECONDS_LIMIT=${CINRS_RA_TIMEOUT:-1800}
VERBOSE=0

say() { printf '%s\n' "$*"; }

for arg in "$@"; do
    case $arg in
    -v | --verbose) VERBOSE=1 ;;
    -h | --help)
        sed -n '3,33p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        printf 'check-rust-analyzer: unknown argument %s (try --help)\n' "$arg" >&2
        exit 2
        ;;
    esac
done

# runs <path> — is this a rust-analyzer that answers?
runs() { [ -n "$1" ] && [ -x "$1" ] && "$1" --version >/dev/null 2>&1; }

# newest <directory> — the newest rust-analyzer under an extensions directory.
#
# The extension directories are named `rust-lang.rust-analyzer-0.3.3049-…`, so a
# version sort picks the newest; `sort -V` is coreutils' and BSD's alike.
newest() {
    local dir=$1 candidate
    [ -d "$dir" ] || return 0
    for candidate in $(ls -d "$dir"/rust-lang.rust-analyzer-* 2>/dev/null | sort -Vr); do
        if runs "$candidate/server/rust-analyzer"; then
            printf '%s\n' "$candidate/server/rust-analyzer"
            return 0
        fi
    done
}

find_rust_analyzer() {
    local found
    if [ -n "${CINRS_RUST_ANALYZER:-}" ]; then
        if runs "$CINRS_RUST_ANALYZER"; then
            printf '%s\n' "$CINRS_RUST_ANALYZER"
            return 0
        fi
        say "check-rust-analyzer: CINRS_RUST_ANALYZER=$CINRS_RUST_ANALYZER does not run" >&2
        return 1
    fi
    found=$(command -v rust-analyzer 2>/dev/null || true)
    if runs "$found"; then
        printf '%s\n' "$found"
        return 0
    fi
    for dir in "$HOME/.vscode-server/extensions" "$HOME/.vscode/extensions"; do
        found=$(newest "$dir")
        if [ -n "$found" ]; then
            printf '%s\n' "$found"
            return 0
        fi
    done
    return 1
}

RA=$(find_rust_analyzer || true)
if [ -z "$RA" ]; then
    say "check-rust-analyzer: skipped: no rust-analyzer binary found."
    say "    Tried \$CINRS_RUST_ANALYZER, 'rust-analyzer' on \$PATH (the rustup proxy of"
    say "    that name errors unless 'rustup component add rust-analyzer' has been run),"
    say "    and the newest rust-lang.rust-analyzer-* under ~/.vscode-server/extensions"
    say "    and ~/.vscode/extensions."
    exit 0
fi

say "check-rust-analyzer: $RA ($("$RA" --version))"
say "check-rust-analyzer: fixture $FIXTURE"

OUT=$(mktemp)
trap 'rm -f "$OUT"' EXIT

# The fixture is a crate of its own, outside the workspace; its artifacts go
# under `target/` like `tests/cross`'s do, so that nothing is written beside the
# sources. rust-analyzer builds the procedural macro itself — that is what makes
# this a test of the host rather than of a library.
status=0
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
    export CARGO_TARGET_DIR=$ROOT/target/rust-analyzer-fixture
    exec timeout -k 10 "$SECONDS_LIMIT" "$RA" diagnostics .
) >"$OUT" 2>&1 || status=$?

if [ "$VERBOSE" -eq 1 ]; then
    say ""
    cat "$OUT"
    say ""
fi

if ! grep -q "diagnostic scan complete" "$OUT"; then
    say "check-rust-analyzer: FAILED: rust-analyzer did not finish (exit $status)"
    tail -30 "$OUT"
    exit 1
fi

# What counts is only what was said about the fixture's own files. Everything
# rust-analyzer reports about the cinrs sources themselves is another matter —
# `inactive_code` on every `#[cfg(test)]` module, above all — and the exit status
# is non-zero whenever *anything* anywhere is an error, so the lines are what this
# reads rather than the status.
#
# Only a *diagnostic* counts, and a diagnostic is text that starts with
# `at crate `. Some versions — the rustup component of 1.98 is one — also write
# progress to the same stream, `3/38 7% processing <path>`, padded with spaces
# and with a carriage return or nothing at all before whatever comes next, so a
# diagnostic may begin in the middle of a line. A progress update names the
# fixture's files too and says nothing about them; it never says `at crate`.
BAD=$(tr '\r' '\n' <"$OUT" | grep -oE 'at crate .*' | grep -F "$FIXTURE" | grep -v "inactive_code" || true)
if [ -n "$BAD" ]; then
    say "check-rust-analyzer: FAILED: rust-analyzer reports the fixture's own code:"
    printf '%s\n' "$BAD"
    say ""
    say "    Every c99! block in the fixture is valid C that 'cargo build' compiles."
    say "    A macro-error here means the text of a block could not be recovered from a"
    say "    host that gives a procedural macro no source positions; see"
    say "    crates/cinrs-core/src/locate.rs and doc/features.md, \"Input forms\"."
    exit 1
fi

say "check-rust-analyzer: ok: nothing reported about the fixture"
