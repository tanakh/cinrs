#!/usr/bin/env bash
#
# Fetches the third-party conformance corpora the optional test harnesses run
# against, into `third_party/`.
#
# Two of them are enormous repositories out of which we want one directory, so
# neither is a submodule: each is a *blobless, sparse* checkout pinned to the
# commit recorded below. `--filter=blob:none` leaves the file contents on the
# server until something asks for them and `sparse-checkout` makes sure only
# the one directory ever does, which turns a multi-gigabyte clone into a few
# tens of megabytes. Both directories are in `.gitignore`.
#
# The third corpus, c-testsuite, *is* a submodule (it is small), and this
# script drives it too so that one command fetches everything.
#
#   scripts/fetch-testsuites.sh            fetch everything that is missing
#   scripts/fetch-testsuites.sh --check    say what is present, fetch nothing
#   scripts/fetch-testsuites.sh gcc llvm   fetch only the named corpora
#
# Re-running it is a no-op when everything is already at its pin, so it is safe
# to put in a CI step in front of the tests.

set -euo pipefail

# --------------------------------------------------------------------------
# the pins
# --------------------------------------------------------------------------
#
# Bump one of these by hand: put the new sha and its author date here, re-run
# the script, and re-run the harness in report mode to refresh the
# expected-failure list, since the corpus itself has moved.

# github.com/gcc-mirror/gcc, master, 2026-09-05.
GCC_COMMIT=3653b8dd68b0caf1c713058854d1efde9cc14329
GCC_DATE=2026-09-05
GCC_URL=https://github.com/gcc-mirror/gcc
# Only the C torture tests: `execute/` is what `tests/gcc_torture.rs` runs.
GCC_PATHS=(gcc/testsuite/gcc.c-torture)

# github.com/llvm/llvm-project, main, 2026-09-04.
LLVM_COMMIT=3efd20d463e1b1210d2ff07cd79b68d6fa215f24
LLVM_DATE=2026-09-04
LLVM_URL=https://github.com/llvm/llvm-project
# Only the standard-conformance tests: `tests/clang_c.rs` runs C99/C11/C23/drs.
LLVM_PATHS=(clang/test/C)

# --------------------------------------------------------------------------

# The package root, whatever directory the script was started from.
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

say() { printf '%s\n' "$*"; }
die() {
    printf 'fetch-testsuites: %s\n' "$*" >&2
    exit 1
}

# fetch_sparse <directory> <url> <commit> <path>...
#
# Leaves `<directory>` a git repository checked out at `<commit>` holding
# nothing but `<path>...`. Every step is idempotent: an existing checkout
# already at the pin is left alone, and one at a different commit (a bumped
# pin) is moved to it without refetching what it already has.
fetch_sparse() {
    local dir=$1 url=$2 commit=$3
    shift 3
    local paths=("$@")

    if [ -e "$dir" ] && [ ! -d "$dir/.git" ]; then
        die "$dir exists and is not a git repository; remove it and re-run"
    fi

    if [ ! -d "$dir/.git" ]; then
        say "fetch-testsuites: creating $dir"
        mkdir -p "$dir"
        git -C "$dir" init --quiet
        git -C "$dir" remote add origin "$url"
    elif [ "$(git -C "$dir" remote get-url origin 2>/dev/null || true)" != "$url" ]; then
        git -C "$dir" remote set-url origin "$url"
    fi

    # `--no-cone` takes the paths as .gitignore-style patterns, which is what
    # lets a single directory be named without listing its parents.
    git -C "$dir" sparse-checkout set --no-cone -- "${paths[@]}"

    if ! git -C "$dir" cat-file -e "$commit^{commit}" 2>/dev/null; then
        say "fetch-testsuites: fetching $url at $commit (blobless, sparse)"
        # A single commit with no history and no file contents; the checkout
        # below then pulls the blobs of the sparse paths and nothing else.
        # GitHub serves an arbitrary sha this way (`uploadpack.allowAnySHA1InWant`).
        git -C "$dir" fetch --quiet --depth 1 --filter=blob:none origin "$commit"
    fi

    if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" != "$commit" ]; then
        say "fetch-testsuites: checking out $commit in $dir"
        git -C "$dir" checkout --quiet --detach "$commit"
    fi
}

# present <directory> <commit> <path>
#
# Prints one `--check` line and returns non-zero when the corpus is not there.
present() {
    local dir=$1 commit=$2 probe=$3 name=$4
    if [ ! -d "$dir/$probe" ]; then
        printf '  %-14s missing   (%s)\n' "$name" "$dir"
        return 1
    fi
    local head
    head=$(git -C "$dir" rev-parse HEAD 2>/dev/null || echo unknown)
    if [ "$head" != "$commit" ]; then
        printf '  %-14s present at %s, pinned to %s — re-run to move it\n' \
            "$name" "${head:0:12}" "${commit:0:12}"
        return 1
    fi
    local files
    files=$(find "$dir/$probe" -name '*.c' | wc -l)
    printf '  %-14s present   %s  (%s .c files, %s)\n' \
        "$name" "${commit:0:12}" "$files" "$(du -sh "$dir" | cut -f1)"
}

fetch_gcc() {
    fetch_sparse third_party/gcc "$GCC_URL" "$GCC_COMMIT" "${GCC_PATHS[@]}"
}

fetch_llvm() {
    fetch_sparse third_party/llvm-project "$LLVM_URL" "$LLVM_COMMIT" "${LLVM_PATHS[@]}"
}

fetch_c_testsuite() {
    if [ ! -f third_party/c-testsuite/tests/single-exec/00001.c ]; then
        say "fetch-testsuites: updating the c-testsuite submodule"
        git submodule update --init third_party/c-testsuite
    fi
}

check() {
    local missing=0
    say "fetch-testsuites: pinned corpora under third_party/"
    present third_party/c-testsuite "$(git submodule status third_party/c-testsuite 2>/dev/null |
        awk '{gsub(/^[-+U]/, "", $1); print $1}')" tests/single-exec c-testsuite || missing=1
    present third_party/gcc "$GCC_COMMIT" gcc/testsuite/gcc.c-torture/execute gcc || missing=1
    present third_party/llvm-project "$LLVM_COMMIT" clang/test/C llvm-project || missing=1
    if [ "$missing" -ne 0 ]; then
        say ""
        say "  run scripts/fetch-testsuites.sh to fetch what is missing"
        return 1
    fi
}

main() {
    local want_check=0
    local -a want=()
    for arg in "$@"; do
        case $arg in
        --check) want_check=1 ;;
        -h | --help)
            sed -n '3,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
            return 0
            ;;
        gcc | llvm | c-testsuite) want+=("$arg") ;;
        *) die "unknown argument $arg (try --help)" ;;
        esac
    done

    if [ "$want_check" -eq 1 ]; then
        check
        return
    fi

    if [ "${#want[@]}" -eq 0 ]; then
        want=(c-testsuite gcc llvm)
    fi
    for corpus in "${want[@]}"; do
        case $corpus in
        c-testsuite) fetch_c_testsuite ;;
        gcc) fetch_gcc ;;
        llvm) fetch_llvm ;;
        esac
    done
    say ""
    check || true
    say ""
    say "fetch-testsuites: pins — gcc $GCC_COMMIT ($GCC_DATE),"
    say "                         llvm-project $LLVM_COMMIT ($LLVM_DATE)"
}

main "$@"
