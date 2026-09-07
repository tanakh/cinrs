//! Integration tests that *run* C11's `<threads.h>` (7.26).
//!
//! The bundled header declares the platform's own thread library — `thrd_*`,
//! `mtx_*`, `cnd_*`, `tss_*` and `call_once` — so these tests do the only
//! thing that can prove it: they create real threads from translated C, take
//! real locks, and check the answers. Two of the types are opaque blocks of
//! bytes whose size belongs to the C library rather than to C, so one test
//! compiles the same header text with the host's `cc` and compares the two
//! reports byte for byte; see [`the_objects_are_the_c_librarys_own`].
//!
//! The whole file is compiled only where the header declares anything: Linux
//! with glibc or musl, which is exactly where `__STDC_NO_THREADS__` is *not*
//! predefined. Elsewhere `#include <threads.h>` is an `#error` naming the
//! reason — `tests/headers.rs` is what checks that — and there is nothing here
//! to run.
#![cfg(all(target_os = "linux", any(target_env = "gnu", target_env = "musl")))]

use std::path::{Path, PathBuf};
use std::process::Command;

use cinrs::{c11, c23};

// ---------------------------------------------------------------------------
// threads, mutexes and joins
// ---------------------------------------------------------------------------

/// Two threads created with `thrd_create`, each raising a counter a mutex
/// protects, both joined: the counter is the sum and nothing was lost.
#[test]
fn two_threads_share_a_mutex_protected_counter() {
    c11! {
        #include <threads.h>

        static mtx_t lock;
        static long counter;

        static int bump(void *arg) {
            long times = *(long *)arg;
            long i;
            for (i = 0; i < times; i++) {
                if (mtx_lock(&lock) != thrd_success) return 1;
                counter++;
                if (mtx_unlock(&lock) != thrd_success) return 2;
            }
            return 0;
        }

        long counted(long times) {
            thrd_t a, b;
            int ra = -1, rb = -1;
            counter = 0;
            if (mtx_init(&lock, mtx_plain) != thrd_success) return -1;
            if (thrd_create(&a, bump, &times) != thrd_success) return -2;
            if (thrd_create(&b, bump, &times) != thrd_success) return -3;
            if (thrd_join(a, &ra) != thrd_success) return -4;
            if (thrd_join(b, &rb) != thrd_success) return -5;
            mtx_destroy(&lock);
            if (ra != 0 || rb != 0) return -6;
            return counter;
        }

        /* Two identifiers for one thread compare equal, and the current
         * thread is not one that has yet to be created. */
        int identity(void) {
            thrd_t self = thrd_current();
            thrd_t again = thrd_current();
            return thrd_equal(self, again) != 0;
        }
    }

    unsafe {
        assert_eq!(counted(10_000), 20_000);
        assert_eq!(identity(), 1);
    }
}

/// `mtx_trylock` on a mutex this thread already holds is `thrd_busy` for a
/// plain mutex and `thrd_success` for a recursive one, and `mtx_timedlock`
/// waits for the deadline and gives up with `thrd_timedout`.
#[test]
fn the_three_kinds_of_lock() {
    c11! {
        #include <threads.h>
        #include <time.h>

        static mtx_t plain_lock;
        static mtx_t recursive_lock;

        static int held(void *unused) {
            (void)unused;
            /* This thread is not the one holding it. */
            return mtx_trylock(&plain_lock);
        }

        int trylock_from_another_thread(void) {
            thrd_t other;
            int result = -1;
            if (mtx_init(&plain_lock, mtx_plain | mtx_timed) != thrd_success) return -1;
            if (mtx_lock(&plain_lock) != thrd_success) return -2;
            if (thrd_create(&other, held, 0) != thrd_success) return -3;
            if (thrd_join(other, &result) != thrd_success) return -4;
            if (mtx_unlock(&plain_lock) != thrd_success) return -5;
            return result;
        }

        static int wait_for_it(void *unused) {
            struct timespec deadline;
            (void)unused;
            timespec_get(&deadline, TIME_UTC);
            /* Long enough that a loaded machine still reaches it, short
             * enough that the test does not hang: the lock is never given
             * up, so this always times out. */
            deadline.tv_nsec += 20 * 1000 * 1000;
            if (deadline.tv_nsec >= 1000000000L) {
                deadline.tv_nsec -= 1000000000L;
                deadline.tv_sec += 1;
            }
            return mtx_timedlock(&plain_lock, &deadline);
        }

        int timedlock_gives_up(void) {
            thrd_t other;
            int result = -1;
            if (mtx_lock(&plain_lock) != thrd_success) return -2;
            if (thrd_create(&other, wait_for_it, 0) != thrd_success) return -3;
            if (thrd_join(other, &result) != thrd_success) return -4;
            if (mtx_unlock(&plain_lock) != thrd_success) return -5;
            mtx_destroy(&plain_lock);
            return result;
        }

        int a_recursive_mutex_locks_twice(void) {
            if (mtx_init(&recursive_lock, mtx_recursive) != thrd_success) return -1;
            if (mtx_lock(&recursive_lock) != thrd_success) return -2;
            if (mtx_trylock(&recursive_lock) != thrd_success) return -3;
            if (mtx_unlock(&recursive_lock) != thrd_success) return -4;
            if (mtx_unlock(&recursive_lock) != thrd_success) return -5;
            mtx_destroy(&recursive_lock);
            return 1;
        }
    }

    unsafe {
        assert_eq!(trylock_from_another_thread(), the_result_codes()[1]); // thrd_busy
        assert_eq!(timedlock_gives_up(), the_result_codes()[4]); // thrd_timedout
        assert_eq!(a_recursive_mutex_locks_twice(), 1);
    }
}

// ---------------------------------------------------------------------------
// call_once
// ---------------------------------------------------------------------------

/// `call_once` runs the function once however many threads reach it, and every
/// one of them is made to wait until it has.
#[test]
fn call_once_runs_the_initializer_exactly_once() {
    c11! {
        #include <threads.h>

        static once_flag flag = ONCE_FLAG_INIT;
        static long runs;
        static long value;

        static void initialize(void) {
            runs++;
            value = 42;
        }

        static int use_it(void *out) {
            call_once(&flag, initialize);
            *(long *)out = value;
            return 0;
        }

        long how_many_runs(void) {
            thrd_t threads[4];
            long seen[4];
            int i;
            for (i = 0; i < 4; i++) {
                seen[i] = 0;
                if (thrd_create(&threads[i], use_it, &seen[i]) != thrd_success) return -1;
            }
            for (i = 0; i < 4; i++) {
                if (thrd_join(threads[i], 0) != thrd_success) return -2;
                /* Every thread saw the initialised value, which is what
                 * `call_once` promises the ones that did not run it. */
                if (seen[i] != 42) return -3;
            }
            return runs;
        }
    }

    assert_eq!(unsafe { how_many_runs() }, 1);
}

// ---------------------------------------------------------------------------
// thread-specific storage
// ---------------------------------------------------------------------------

/// One `tss_t` key, one value per thread, and the destructor run when a thread
/// that set one ends.
#[test]
fn thread_specific_storage_is_private_to_each_thread() {
    c11! {
        #include <threads.h>

        static tss_t key;
        static mtx_t destructor_lock;
        static long destroyed;

        static void forget(void *value) {
            mtx_lock(&destructor_lock);
            destroyed += *(long *)value;
            mtx_unlock(&destructor_lock);
        }

        static long slots[2];

        static int store(void *arg) {
            long *slot = (long *)arg;
            if (tss_get(key) != 0) return 1;      /* a new thread starts empty */
            if (tss_set(key, slot) != thrd_success) return 2;
            if (tss_get(key) != slot) return 3;   /* and reads back its own */
            return 0;
        }

        long per_thread_values(void) {
            thrd_t a, b;
            int ra = -1, rb = -1;
            destroyed = 0;
            slots[0] = 3;
            slots[1] = 4;
            if (mtx_init(&destructor_lock, mtx_plain) != thrd_success) return -1;
            if (tss_create(&key, forget) != thrd_success) return -2;
            if (tss_set(key, 0) != thrd_success) return -3;
            if (thrd_create(&a, store, &slots[0]) != thrd_success) return -4;
            if (thrd_join(a, &ra) != thrd_success) return -5;
            if (thrd_create(&b, store, &slots[1]) != thrd_success) return -6;
            if (thrd_join(b, &rb) != thrd_success) return -7;
            if (ra != 0 || rb != 0) return -8;
            /* This thread's own value was never touched by either of them. */
            if (tss_get(key) != 0) return -9;
            tss_delete(key);
            mtx_destroy(&destructor_lock);
            return destroyed;
        }
    }

    // Each thread's destructor ran with that thread's own value: 3 and 4.
    assert_eq!(unsafe { per_thread_values() }, 7);
}

// ---------------------------------------------------------------------------
// condition variables
// ---------------------------------------------------------------------------

/// A handshake: the second thread waits on a condition variable until the
/// first signals it, and `cnd_timedwait` gives up on a deadline nothing
/// reaches.
#[test]
fn a_condition_variable_handshake() {
    c11! {
        #include <threads.h>
        #include <time.h>

        static mtx_t lock;
        static cnd_t ready;
        static long state;

        static int waiter(void *out) {
            if (mtx_lock(&lock) != thrd_success) return 1;
            while (state == 0) {
                if (cnd_wait(&ready, &lock) != thrd_success) {
                    mtx_unlock(&lock);
                    return 2;
                }
            }
            *(long *)out = state;
            state = 0;
            if (mtx_unlock(&lock) != thrd_success) return 3;
            return 0;
        }

        long handshake(long token) {
            thrd_t other;
            long seen = -1;
            int result = -1;
            state = 0;
            if (mtx_init(&lock, mtx_plain) != thrd_success) return -1;
            if (cnd_init(&ready) != thrd_success) return -2;
            if (thrd_create(&other, waiter, &seen) != thrd_success) return -3;
            if (mtx_lock(&lock) != thrd_success) return -4;
            state = token;
            if (cnd_signal(&ready) != thrd_success) return -5;
            if (mtx_unlock(&lock) != thrd_success) return -6;
            if (thrd_join(other, &result) != thrd_success) return -7;
            if (result != 0) return -8 - result;
            return seen;
        }

        /* Like `waiter`, but it leaves `state` alone, so every waiter sees the
         * same token rather than taking it from the others. */
        static int patient(void *out) {
            if (mtx_lock(&lock) != thrd_success) return 1;
            while (state == 0) {
                if (cnd_wait(&ready, &lock) != thrd_success) {
                    mtx_unlock(&lock);
                    return 2;
                }
            }
            *(long *)out = state;
            if (mtx_unlock(&lock) != thrd_success) return 3;
            return 0;
        }

        int broadcast_wakes_every_waiter(void) {
            thrd_t waiters[3];
            long seen[3];
            int i;
            state = 0;
            for (i = 0; i < 3; i++) {
                seen[i] = 0;
                if (thrd_create(&waiters[i], patient, &seen[i]) != thrd_success) return -1;
            }
            /* One token and one `cnd_broadcast`: all three wake on it, which
             * is what `cnd_signal` would not promise. */
            if (mtx_lock(&lock) != thrd_success) return -2;
            state = 7;
            if (cnd_broadcast(&ready) != thrd_success) return -3;
            if (mtx_unlock(&lock) != thrd_success) return -4;
            for (i = 0; i < 3; i++) {
                if (thrd_join(waiters[i], 0) != thrd_success) return -5;
                if (seen[i] != 7) return -6;
            }
            state = 0;
            return 1;
        }

        int timedwait_gives_up(void) {
            struct timespec deadline;
            int result;
            if (mtx_lock(&lock) != thrd_success) return -1;
            if (timespec_get(&deadline, TIME_UTC) != TIME_UTC) return -2;
            deadline.tv_nsec += 20 * 1000 * 1000;
            if (deadline.tv_nsec >= 1000000000L) {
                deadline.tv_nsec -= 1000000000L;
                deadline.tv_sec += 1;
            }
            /* Nothing signals it, so the only way out is the deadline. */
            result = cnd_timedwait(&ready, &lock, &deadline);
            if (mtx_unlock(&lock) != thrd_success) return -3;
            return result;
        }

        void done(void) {
            cnd_destroy(&ready);
            mtx_destroy(&lock);
        }
    }

    unsafe {
        assert_eq!(handshake(99), 99);
        assert_eq!(broadcast_wakes_every_waiter(), 1);
        assert_eq!(timedwait_gives_up(), the_result_codes()[4]); // thrd_timedout
        done();
    }
}

// ---------------------------------------------------------------------------
// thrd_sleep, thrd_yield and thrd_detach
// ---------------------------------------------------------------------------

/// `thrd_sleep` takes a `struct timespec` from `<time.h>` and really waits.
#[test]
fn thrd_sleep_waits_for_a_timespec() {
    c11! {
        #include <threads.h>
        #include <time.h>

        int nap(long nanoseconds) {
            struct timespec how_long;
            how_long.tv_sec = 0;
            how_long.tv_nsec = nanoseconds;
            /* Zero on success; a negative value if a signal cut it short,
             * which nothing here sends. */
            return thrd_sleep(&how_long, 0);
        }

        int yielding(void) {
            thrd_yield();
            return 1;
        }

        static int shortlived(void *unused) {
            (void)unused;
            return 0;
        }

        int detaching(void) {
            thrd_t other;
            if (thrd_create(&other, shortlived, 0) != thrd_success) return -1;
            /* Nothing joins it, and detaching is what says so. */
            return thrd_detach(other) == thrd_success;
        }
    }

    let start = std::time::Instant::now();
    unsafe {
        assert_eq!(nap(5_000_000), 0);
        assert_eq!(yielding(), 1);
        assert_eq!(detaching(), 1);
    }
    assert!(
        start.elapsed() >= std::time::Duration::from_millis(4),
        "thrd_sleep returned before the five milliseconds were up"
    );
}

/// `thrd_exit` is declared `_Noreturn`, which is what lets a function that
/// ends in a call to it return a value without a `return`.
///
/// It is deliberately never *called*: `thrd_exit` is `pthread_exit`, which
/// ends the thread by forcing an unwind through the frames above it, and the
/// frame above it here is generated Rust. Rust aborts rather than let a
/// foreign unwind cross an `extern "C"` frame, so a thread that called it
/// would take the whole test process with it. This is what the header can
/// honestly promise: the declaration, and that C which ends in it compiles.
#[test]
fn thrd_exit_is_noreturn() {
    c11! {
        #include <threads.h>

        /* No `return` after it, and the function is not `void`: this compiles
         * only because the call cannot come back. */
        static int leave(void *code) {
            thrd_exit(*(int *)code);
        }

        /* The address is taken, so the function is generated and its body is
         * really compiled, but nothing here calls it. */
        thrd_start_t the_exiting_thread(void) { return leave; }
    }

    assert!(unsafe { the_exiting_thread() }.is_some());
}

// ---------------------------------------------------------------------------
// the constants, and `thread_local`
// ---------------------------------------------------------------------------

/// The five `thrd_*` result codes, in the order the tests above index them:
/// success, busy, error, nomem, timedout. That they are the C library's own
/// values is [`the_objects_are_the_c_librarys_own`]'s business; this is only
/// how a test names one.
fn the_result_codes() -> [i32; 5] {
    c11! {
        #include <threads.h>

        void thrd_codes(int *out) {
            out[0] = thrd_success;
            out[1] = thrd_busy;
            out[2] = thrd_error;
            out[3] = thrd_nomem;
            out[4] = thrd_timedout;
        }
    }

    let mut codes = [-1i32; 5];
    unsafe { thrd_codes(codes.as_mut_ptr()) };
    codes
}

/// C11 7.26.1p3 makes `thread_local` a macro for `_Thread_local`; C23 (N2934)
/// made it a keyword, and the header must not define the macro there.
#[test]
fn thread_local_is_a_macro_in_c11_and_a_keyword_in_c23() {
    c11! {
        #include <threads.h>

        thread_local int per_thread_c11 = 5;

        int c11_macro_is_defined(void) {
        #ifdef thread_local
            return per_thread_c11;
        #else
            return 0;
        #endif
        }
    }

    c23! {
        #include <threads.h>

        thread_local int per_thread_c23 = 6;

        int c23_keyword_needs_no_macro(void) {
        #ifdef thread_local
            return 0;
        #else
            return per_thread_c23;
        #endif
        }
    }

    unsafe {
        assert_eq!(c11_macro_is_defined(), 5);
        assert_eq!(c23_keyword_needs_no_macro(), 6);
    }
}

/// `__STDC_NO_THREADS__` is C11 6.10.8.3's way of saying the thread library is
/// absent. It must not be predefined here, where the header works.
#[test]
fn the_subsetting_macro_is_not_defined_where_the_header_works() {
    c11! {
        int threads_are_here(void) {
        #ifdef __STDC_NO_THREADS__
            return 0;
        #else
            return 1;
        #endif
        }
    }

    assert_eq!(unsafe { threads_are_here() }, 1);
}

// ---------------------------------------------------------------------------
// the layouts, differentially against the host's own C compiler
// ---------------------------------------------------------------------------

c11! {
    #include "include/threads_probe.h"
}

/// The `main` handed to the host compiler.
///
/// It prints the shared report and then, from `<pthread.h>`, the sizes of the
/// two objects `mtx_t` and `cnd_t` *are* on this platform — which is the claim
/// the bundled header makes and the one worth writing down.
const MAIN_C: &str = r#"#include <pthread.h>
#include <stdio.h>
#include "threads_probe.h"

static char buffer[1 << 16];

int main(void) {
    int n = thr_report(buffer);
    fwrite(buffer, 1, (size_t) n, stdout);
    printf("pthread_mutex_t %d %d\n",
           (int) sizeof(pthread_mutex_t), (int) _Alignof(pthread_mutex_t));
    printf("pthread_cond_t %d %d\n",
           (int) sizeof(pthread_cond_t), (int) _Alignof(pthread_cond_t));
    return 0;
}
"#;

/// Every size, alignment and constant the bundled `<threads.h>` declares is
/// the C library's own on this machine — and `mtx_t` and `cnd_t` really are
/// `pthread_mutex_t` and `pthread_cond_t`.
///
/// Without a C compiler on `PATH` there is nothing to compare against, and the
/// test says why and passes. `CINRS_THREADS_CC` names the compiler, `CC` is
/// consulted next, and `cc` is the default.
#[test]
fn the_objects_are_the_c_librarys_own() {
    let Some(cc) = c_compiler() else {
        println!("no C compiler on PATH: skipping the differential layout check");
        return;
    };

    let mine = report_through_cinrs();
    let theirs = report_through(&cc);
    let (shared, extra) = split_report(&theirs);
    assert_eq!(
        mine, shared,
        "the bundled <threads.h> and {cc}'s do not describe the same objects"
    );

    // `mtx_t` is `pthread_mutex_t` and `cnd_t` is `pthread_cond_t`; the line
    // for each says so with the same two numbers.
    for (theirs, ours) in [("pthread_mutex_t", "mtx_t"), ("pthread_cond_t", "cnd_t")] {
        assert_eq!(
            numbers(&extra, theirs),
            numbers(&mine, ours),
            "{ours} is not the same object as {theirs}"
        );
    }
}

/// The report as the translated header produces it.
fn report_through_cinrs() -> String {
    let mut buffer = vec![0u8; 1 << 16];
    let written = unsafe { thr_report(buffer.as_mut_ptr().cast()) };
    let len = usize::try_from(written).expect("the report cannot be negative");
    assert!(len < buffer.len(), "the report buffer was too small");
    String::from_utf8(buffer[..len].to_vec()).expect("the report is ASCII")
}

/// The report as the host compiler produces it, with its `<pthread.h>` tail.
fn report_through(cc: &str) -> String {
    let dir = manifest_dir().join("target/c11-threads");
    std::fs::create_dir_all(&dir).expect("target/ must be writable");
    let source = dir.join("main.c");
    std::fs::write(&source, MAIN_C).expect("target/ must be writable");
    let binary = dir.join("threads_report");

    let output = Command::new(cc)
        .args(["-std=c11", "-w", "-O0"])
        .arg("-I")
        .arg(manifest_dir().join("tests/include"))
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap_or_else(|e| panic!("could not run {cc}: {e}"));
    assert!(
        output.status.success(),
        "{cc} could not compile the probe:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    run(&binary)
}

/// The shared report and the `<pthread.h>` lines `main.c` adds after it.
fn split_report(report: &str) -> (String, String) {
    let at = report
        .find("pthread_mutex_t ")
        .expect("the C side prints the pthread lines last");
    (report[..at].to_owned(), report[at..].to_owned())
}

/// The numbers on the line of `report` that starts with `key`.
fn numbers(report: &str, key: &str) -> Vec<i64> {
    let line = report
        .lines()
        .find(|line| line.split(' ').next() == Some(key))
        .unwrap_or_else(|| panic!("no '{key}' line in\n{report}"));
    line.split(' ')
        .skip(1)
        .map(|n| n.parse().expect("the report is numbers"))
        .collect()
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The compiler to compare against, if there is one.
fn c_compiler() -> Option<String> {
    let named = std::env::var("CINRS_THREADS_CC")
        .or_else(|_| std::env::var("CC"))
        .unwrap_or_else(|_| "cc".to_owned());
    let ok = Command::new(&named)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    ok.then_some(named)
}

fn run(binary: &Path) -> String {
    let output = Command::new(binary)
        .output()
        .unwrap_or_else(|e| panic!("could not run {}: {e}", binary.display()));
    assert!(
        output.status.success(),
        "{} exited with {}",
        binary.display(),
        output.status
    );
    String::from_utf8(output.stdout).expect("the report is ASCII")
}
