# shellcheck shell=bash
#
# What `--bench` shares in scripts/check-sqlite.sh, check-blake3.sh and
# check-xxhash.sh. Sourced, not run.
#
# Each of those scripts builds the upstream C natively into static libraries
# under `target/<program>/native/` — with `gcc -O2` and `clang -O2`, and the
# per-file flags upstream's own build uses — then runs its fixture's
# `tests/bench.rs` three ways, one cargo command at a time: the C cinrs
# translated (`--release`), and the same Rust linked against each native
# library (`--features native-gcc`, `--features native-clang`). This file is
# the part that is the same for all three: the ceilings, the compiler
# invocations, the cargo runs and the table.
#
# The caller sets, before using anything here:
#
#   NB_NAME        the script's name, for messages (`check-blake3`)
#   FIXTURE        the fixture crate
#   NB_TARGET_DIR  its CARGO_TARGET_DIR
#   NATIVE         where the libraries and the runs' output go
#   CARGO          the cargo command, an array (`cargo +beta`)
#   VMEM_KB        the address-space ceiling in KB, `0` for none
#   SECONDS_LIMIT  the clock ceiling per step
#
# and has `say` defined.

# A command under both ceilings.
nb_limited() {
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
        exec timeout -k 10 "$SECONDS_LIMIT" "$@"
    )
}

# The compilers that are installed, of gcc and clang, in NB_CCS.
NB_CCS=()
nb_find_compilers() {
    local cc
    NB_CCS=()
    for cc in gcc clang; do
        if command -v "$cc" >/dev/null 2>&1; then
            NB_CCS+=("$cc")
        else
            say "$NB_NAME: note: $cc is not installed; its column is left empty"
        fi
    done
}

# nb_compile CC SOURCE OBJECT [FLAGS…]: `CC -O2 -std=gnu11 FLAGS… -c SOURCE`.
nb_compile() {
    local cc=$1 src=$2 obj=$3
    shift 3
    say "    $cc -O2 -std=gnu11 $* -c ${src#"$ROOT"/}"
    nb_limited "$cc" -O2 -std=gnu11 "$@" -c "$src" -o "$obj"
}

# nb_archive LIBRARY OBJECT…: a fresh `ar rcs`.
nb_archive() {
    local lib=$1
    shift
    rm -f "$lib"
    ar rcs "$lib" "$@"
    say "    ar rcs ${lib#"$ROOT"/}"
}

# nb_run LABEL [FEATURE]: one `cargo test --release --test bench`, its output
# shown and kept in $NATIVE/bench-LABEL.out.
nb_run() {
    local label=$1 feature=${2:-} out=$NATIVE/bench-$1.out
    local args=(test --release --test bench)
    if [ -n "$feature" ]; then
        args+=(--features "$feature")
    fi
    # The bench is `#[ignore]`d so that the check's plain `cargo test` skips it.
    args+=(-- --ignored --nocapture --test-threads=1)
    say ""
    say "=== bench, $label: ${CARGO[*]} ${args[*]} ==="
    local status=0
    (
        cd "$FIXTURE"
        export CARGO_TARGET_DIR=$NB_TARGET_DIR
        nb_limited "${CARGO[@]}" "${args[@]}"
    ) 2>&1 | tee "$out" || status=$?
    if [ "$status" -ne 0 ]; then
        say "$NB_NAME: FAILED: bench, $label (exit $status)"
        return 1
    fi
    return 0
}

# The rows (`section, ms`) and checksums a run printed.
nb_rows() { sed -n 's/^.*bench-row: //p' "$NATIVE/bench-$1.out" 2>/dev/null; }
nb_sums() { sed -n 's/^.*bench-checksum: //p' "$NATIVE/bench-$1.out" 2>/dev/null; }
nb_info() { sed -n 's/^.*bench-info: //p' "$NATIVE/bench-$1.out" 2>/dev/null; }

# nb_ms LABEL SECTION: that run's number for that section, or nothing.
nb_ms() {
    nb_rows "$1" | awk -v s="$2" '{ i = index($0, ", "); if (substr($0, 1, i - 1) == s) print substr($0, i + 2) }'
}

nb_ratio() {
    if [ -z "$1" ] || [ -z "$2" ]; then
        printf '%s' "-"
        return
    fi
    awk -v a="$1" -v b="$2" 'BEGIN { if (b > 0) printf "%.2f", a / b; else printf "-" }'
}

# nb_report LABEL…: the table, the checksums, the versions. Fails when a run
# printed no rows or when the checksums of the runs differ. The first label is
# cinrs's run; the others are the native ones that ran.
nb_report() {
    local ran=("$@") label section ms g c status=0
    say ""
    say "=== $NB_NAME --bench: results ==="
    say ""
    for label in "${ran[@]}"; do
        if [ -z "$(nb_rows "$label")" ]; then
            say "$NB_NAME: FAILED: the $label run printed no bench-row lines"
            status=1
        fi
    done
    say "| section | gcc ms | clang ms | cinrs ms | cinrs/gcc | cinrs/clang |"
    say "|---|---:|---:|---:|---:|---:|"
    while IFS= read -r row; do
        section=${row%, *}
        ms=${row##*, }
        g=$(nb_ms gcc "$section")
        c=$(nb_ms clang "$section")
        say "| $section | ${g:--} | ${c:--} | $ms | $(nb_ratio "$ms" "$g") | $(nb_ratio "$ms" "$c") |"
    done < <(nb_rows cinrs)
    say ""
    say "checksums (cinrs):"
    nb_sums cinrs | sed 's/^/    /'
    for label in "${ran[@]:1}"; do
        if [ "$(nb_sums "$label")" = "$(nb_sums cinrs)" ]; then
            say "    $label: identical"
        else
            say "$NB_NAME: FAILED: the $label run's checksums differ from cinrs's:"
            diff <(nb_sums cinrs) <(nb_sums "$label") | sed 's/^/    /' || true
            status=1
        fi
    done
    say ""
    for label in "${ran[@]}"; do
        nb_info "$label" | sed "s/^/$label: /"
    done
    say ""
    say "date:  $(date '+%Y-%m-%d %H:%M %Z')"
    say "host:  $(sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2>/dev/null | head -1), $(uname -sr)"
    for label in "${NB_CCS[@]}"; do
        say "$label: $("$label" --version | head -1)"
    done
    say "rust:  $(rustc "${CARGO[@]:1}" --version)"
    say "cinrs: release profile; natives: -O2 -std=gnu11, flags as upstream builds each file"
    return "$status"
}
