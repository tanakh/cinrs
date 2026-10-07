//! A thread-local object with external linkage, defined in one unit and used
//! from another.
//!
//! Rust has no stable way to name a TLS symbol another object defines, so the
//! defining unit — which gives the object a C symbol, here with `#pragma cinrs
//! export` — exports an accessor, `name.cinrs_tls`, that returns the calling
//! thread's copy, and a unit that says `extern _Thread_local T name;` reaches
//! the object through it. Each thread still sees its own copy.

mod definer {
    cinrs::c11! {
        #pragma cinrs export
        _Thread_local int cinrs_tls_test_counter = 5;
        _Thread_local long cinrs_tls_test_total;
        int cinrs_tls_test_bump(int by) {
            cinrs_tls_test_counter += by;
            return cinrs_tls_test_counter;
        }
    }
}

mod user {
    cinrs::gnu11! {
        extern __thread int cinrs_tls_test_counter;
        extern _Thread_local long cinrs_tls_test_total;
        int cinrs_tls_test_bump(int by);

        int run(int start) {
            int *mine = &cinrs_tls_test_counter;
            cinrs_tls_test_counter = start;
            for (int i = 0; i < 1000; i++) cinrs_tls_test_bump(1);
            cinrs_tls_test_total += *mine;
            return cinrs_tls_test_counter + (int)cinrs_tls_test_total;
        }
    }
}

#[test]
fn each_thread_sees_its_own_copy_through_the_accessor() {
    let threads: Vec<_> = [100, 200, 300]
        .into_iter()
        .map(|start| std::thread::spawn(move || unsafe { user::run(start) }))
        .collect();
    let results: Vec<i32> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    // Each thread starts from its own value and its own zero total.
    assert_eq!(results, [2200, 2400, 2600]);
    // This thread's copy was never touched by the others: still the
    // initialiser's 5.
    assert_eq!(unsafe { definer::cinrs_tls_test_bump(0) }, 5);
}
