#!/usr/bin/env bash
#
# Does ccinrs build real C projects through their own build systems?
#
#   scripts/check-ccinrs.sh                 everything below
#   scripts/check-ccinrs.sh lz4 cmark       only the ones named (lz4, cjson,
#                                           cmark, brotli, c-testsuite)
#   scripts/check-ccinrs.sh --keep          keep the downloaded tarballs
#
# ccinrs is meant to be dropped in where a build expects GCC: `make CC=ccinrs`,
# or CMake told it is the C compiler. This builds four projects that way, from
# pinned release tarballs checked against their SHA-256, with ccinrs's
# run-time checks on as they are by default, and runs each project's own
# tests:
#
#   lz4 1.10.0    `make`: the library, static and shared, and the `lz4`
#                 command; the frame and fuzz tests, and the command's tests
#   cJSON 1.7.19  `make` (serially: upstream's rule for libcjson_utils.so uses
#                 cJSON.o without depending on it): both libraries, static and
#                 shared, and the test program; then the 21 Unity unit tests,
#                 built by hand with Unity's own switches for a compiler
#                 without setjmp/longjmp or weak definitions
#                 (UNITY_EXCLUDE_SETJMP_H, UNITY_NO_WEAK)
#   cmark 0.31.2  CMake, the `cmark` command; the CommonMark spec, the
#                 regression and the smart-punctuation tests (python3).
#                 `api_test` is not built: it has C++ in it, which CMake links
#                 with `c++`, and only `rustc` can link ccinrs's objects
#   brotli 1.2.0  CMake, the shared libraries and the command; ctest
#
# and c-testsuite (the third_party/c-testsuite submodule) through the command
# line, `ccinrs case.c -o case && ./case`, whose failures must all be on
# tests/c-testsuite/expected-failures.txt.
#
# The projects are unpacked afresh under target/ccinrs-check/ and nothing of
# them is kept in this repository; doc/ccinrs.md has the results. ccinrs is
# built in release mode first, and keeps the runtime it compiles in
# target/ccinrs-check/cache. Every step runs under a ceiling on the address
# space and one on the clock: `CCINRS_CHECK_ULIMIT_V` (kilobytes, 8000000 by
# default, `0` switches it off), `CCINRS_CHECK_TIMEOUT` (seconds per step,
# 1800) and `CCINRS_CHECK_JOBS` (make's -j, 4). A step whose tool is missing
# (cmake, python3) is skipped with a note.

set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

WORK=$ROOT/target/ccinrs-check
VMEM_KB=${CCINRS_CHECK_ULIMIT_V:-8000000}
SECONDS_LIMIT=${CCINRS_CHECK_TIMEOUT:-1800}
JOBS=${CCINRS_CHECK_JOBS:-4}
KEEP=0
ONLY=()
FAILED=()

say() { printf '%s\n' "$*"; }

for arg in "$@"; do
    case $arg in
    -k | --keep) KEEP=1 ;;
    -h | --help)
        sed -n '3,42p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    lz4 | cjson | cmark | brotli | c-testsuite) ONLY+=("$arg") ;;
    *)
        printf 'check-ccinrs: unknown argument %s (try --help)\n' "$arg" >&2
        exit 2
        ;;
    esac
done

wanted() {
    [ "${#ONLY[@]}" -eq 0 ] && return 0
    local name
    for name in "${ONLY[@]}"; do
        [ "$name" = "$1" ] && return 0
    done
    return 1
}

# step <label> -- <command>...
#
# Runs one command under the two ceilings, in a subshell so that the limit is
# gone again afterwards, and records a failure rather than stopping.
step() {
    local label=$1
    shift
    say ""
    say "=== $label ==="
    (
        if [ "$VMEM_KB" != 0 ]; then
            ulimit -v "$VMEM_KB" 2>/dev/null || true
        fi
        exec timeout -k 10 "$SECONDS_LIMIT" "$@"
    ) || {
        say "check-ccinrs: FAILED ($?): $label"
        FAILED+=("$label")
        return 0
    }
    say "check-ccinrs: ok: $label"
}

# ---------------------------------------------------------------------------
# ccinrs itself
# ---------------------------------------------------------------------------

cargo build --release --locked -p ccinrs
CCINRS=$ROOT/target/release/ccinrs
export CCINRS_CACHE_DIR=$WORK/cache
mkdir -p "$WORK/src"
say "check-ccinrs: $("$CCINRS" --version | head -1)"

# ---------------------------------------------------------------------------
# what is downloaded, and how it is checked
# ---------------------------------------------------------------------------

fetch() {
    local url=$1 out=$2
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$out" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$out" "$url"
    else
        say "check-ccinrs: neither curl nor wget is installed; cannot fetch $url"
        return 1
    fi
}

# unpack <name> <url> <sha256> <directory in the tarball>
#
# Leaves a fresh copy of the project in $WORK/src/<directory>; prints that
# directory's path.
unpack() {
    local name=$1 url=$2 sha=$3 dir=$4
    local tarball=$WORK/$name.tar.gz got
    if [ ! -f "$tarball" ] || [ "$(sha256sum "$tarball" | cut -d' ' -f1)" != "$sha" ]; then
        fetch "$url" "$tarball" >&2 || return 1
    fi
    got=$(sha256sum "$tarball" | cut -d' ' -f1)
    if [ "$got" != "$sha" ]; then
        say "check-ccinrs: $url is not the file pinned here (sha256 $got, expected $sha)" >&2
        return 1
    fi
    rm -rf "${WORK:?}/src/$dir"
    tar xzf "$tarball" -C "$WORK/src"
    [ "$KEEP" -eq 1 ] || rm -f "$tarball"
    printf '%s\n' "$WORK/src/$dir"
}

have() {
    command -v "$1" >/dev/null 2>&1 && return 0
    say ""
    say "check-ccinrs: skipped $2: $1 is not installed"
    return 1
}

# ---------------------------------------------------------------------------
# lz4
# ---------------------------------------------------------------------------

check_lz4() {
    local src
    src=$(unpack lz4 \
        https://github.com/lz4/lz4/releases/download/v1.10.0/lz4-1.10.0.tar.gz \
        537512904744b35e232912055ccf8ec66d768639ff3abe5788d90d792ec5f48b \
        lz4-1.10.0) || {
        FAILED+=("lz4: fetch")
        return
    }
    step "lz4: make" make -C "$src" -j"$JOBS" CC="$CCINRS"
    step "lz4: frametest and fuzzer" sh -c "
        make -C '$src/tests' -j$JOBS CC='$CCINRS' frametest fuzzer &&
        '$src/tests/frametest' -i100 && '$src/tests/fuzzer' -i50"
    step "lz4: the command's tests" make -C "$src/tests" CC="$CCINRS" \
        test-lz4-basic test-lz4-dict test-lz4-multiple test-lz4-sparse \
        test-lz4-frame-concatenation test-lz4-opt-parser test-lz4-essentials
}

# ---------------------------------------------------------------------------
# cJSON
# ---------------------------------------------------------------------------

check_cjson() {
    local src
    src=$(unpack cjson \
        https://github.com/DaveGamble/cJSON/archive/refs/tags/v1.7.19.tar.gz \
        7fa616e3046edfa7a28a32d5f9eacfd23f92900fe1f8ccd988c1662f30454562 \
        cJSON-1.7.19) || {
        FAILED+=("cjson: fetch")
        return
    }
    step "cjson: make" make -C "$src" CC="$CCINRS"
    step "cjson: cJSON_test" sh -c "cd '$src' && ./cJSON_test | head -1 | grep -qx 'Version: 1.7.19'"
    step "cjson: the Unity unit tests" sh -c '
        set -e
        cc=$1
        cd "$2/tests"
        unity="-DUNITY_EXCLUDE_SETJMP_H -DUNITY_NO_WEAK -Iunity/src"
        mkdir -p ../unit
        $cc -c $unity unity/src/unity.c -o ../unit/unity.o
        $cc -c unity_setup.c -o ../unit/unity_setup.o
        $cc -c -I.. ../cJSON_Utils.c -o ../unit/cJSON_Utils.o
        failed=0
        for test in *.c; do
            name=${test%.c}
            case $name in unity_setup | common) continue ;; esac
            # The tests #include cJSON.c; the utilities ones need cJSON_Utils.
            extra=
            if grep -q cJSON_Utils "$test" && ! grep -q cJSON_Utils.c "$test"; then
                extra=../unit/cJSON_Utils.o
            fi
            if $cc -w $unity -I.. "$test" $extra ../unit/unity.o ../unit/unity_setup.o \
                -lm -o "../unit/$name" && "../unit/$name" > "../unit/$name.out"; then
                echo "  $name: $(tail -2 "../unit/$name.out" | head -1)"
            else
                echo "  $name: FAILED"
                failed=1
            fi
        done
        exit $failed' sh "$CCINRS" "$src"
}

# ---------------------------------------------------------------------------
# cmark
# ---------------------------------------------------------------------------

check_cmark() {
    have cmake cmark || return 0
    local src
    src=$(unpack cmark \
        https://github.com/commonmark/cmark/archive/refs/tags/0.31.2.tar.gz \
        f9bc5ca38bcb0b727f0056100fac4d743e768872e3bacec7746de28f5700d697 \
        cmark-0.31.2) || {
        FAILED+=("cmark: fetch")
        return
    }
    step "cmark: cmake" sh -c "
        cmake -S '$src' -B '$src/build' -DCMAKE_C_COMPILER='$CCINRS' \
            -DCMAKE_BUILD_TYPE=Release > '$src/build.log' &&
        cmake --build '$src/build' -j$JOBS --target cmark_exe"
    have python3 "cmark's spec tests" || return 0
    local spec
    for spec in spec regression smart_punct; do
        local program="$src/build/src/cmark"
        [ "$spec" = smart_punct ] && program="$program --smart"
        step "cmark: test/$spec.txt" sh -c "
            cd '$src' &&
            result=\$(python3 test/spec_tests.py --spec test/$spec.txt --program '$program' | tail -1) &&
            echo \"\$result\" &&
            case \$result in *' 0 failed, 0 errored'*) ;; *) exit 1 ;; esac"
    done
}

# ---------------------------------------------------------------------------
# brotli
# ---------------------------------------------------------------------------

check_brotli() {
    have cmake brotli || return 0
    local src
    src=$(unpack brotli \
        https://github.com/google/brotli/archive/refs/tags/v1.2.0.tar.gz \
        816c96e8e8f193b40151dad7e8ff37b1221d019dbcb9c35cd3fadbfe6477dfec \
        brotli-1.2.0) || {
        FAILED+=("brotli: fetch")
        return
    }
    step "brotli: cmake" sh -c "
        cmake -S '$src' -B '$src/build' -DCMAKE_C_COMPILER='$CCINRS' \
            -DCMAKE_BUILD_TYPE=Release > '$src/build.log' &&
        cmake --build '$src/build' -j$JOBS"
    step "brotli: ctest" sh -c "cd '$src/build' && ctest -j$JOBS --output-on-failure"
}

# ---------------------------------------------------------------------------
# c-testsuite, through the command line
# ---------------------------------------------------------------------------

check_c_testsuite() {
    local suite=$ROOT/third_party/c-testsuite/tests/single-exec
    if [ ! -d "$suite" ]; then
        say ""
        say "check-ccinrs: skipped c-testsuite: the submodule is not checked out" \
            "(git submodule update --init)"
        return 0
    fi
    local out=$WORK/c-testsuite
    rm -rf "$out"
    mkdir -p "$out"
    step "c-testsuite" sh -c '
        cc=$1 suite=$2 out=$3 list=$4 jobs=$5
        ls "$suite"/*.c | xargs -P "$jobs" -I{} sh -c '\''
            c=$1 cc=$2 out=$3
            name=$(basename "$c" .c)
            if ! "$cc" -w "$c" -o "$out/$name" > "$out/$name.err" 2>&1; then
                echo "$name compile"
            elif ! (cd "$out" && timeout 30 "./$name" > "$name.out" 2>&1); then
                echo "$name run"
            elif ! cmp -s "$out/$name.out" "$c.expected"; then
                echo "$name output"
            else
                echo "$name ok"
            fi'\'' sh {} "$cc" "$out" > "$out/results.txt"
        total=$(wc -l < "$out/results.txt")
        passed=$(grep -c " ok$" "$out/results.txt" || true)
        echo "$passed of $total pass"
        # A plain line of the list is a known failure; anything else that
        # failed is new.
        unexpected=0
        for failure in $(grep -v " ok$" "$out/results.txt" | cut -d" " -f1 | sort); do
            how=$(grep "^$failure " "$out/results.txt" | cut -d" " -f2)
            if grep -Eq "^$failure[[:space:]]" "$list"; then
                echo "  $failure: fails to $how, as the list says"
            else
                echo "  $failure: FAILED to $how, and is not on the list"
                unexpected=1
            fi
        done
        exit $unexpected' sh "$CCINRS" "$suite" "$out" \
        "$ROOT/tests/c-testsuite/expected-failures.txt" "$JOBS"
}

wanted lz4 && check_lz4
wanted cjson && check_cjson
wanted cmark && check_cmark
wanted brotli && check_brotli
wanted c-testsuite && check_c_testsuite

say ""
if [ "${#FAILED[@]}" -eq 0 ]; then
    say "check-ccinrs: everything passed"
else
    say "check-ccinrs: ${#FAILED[@]} step(s) failed:"
    for label in "${FAILED[@]}"; do
        say "  - $label"
    done
    exit 1
fi
