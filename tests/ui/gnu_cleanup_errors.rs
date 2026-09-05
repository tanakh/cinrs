//! What `__attribute__((cleanup(f)))` may not be written on, and what it may
//! not name.
//!
//! GCC drops the attribute with "'cleanup' attribute ignored" wherever the
//! object's scope is not a scope a call could hang on; ignoring it here would
//! change what the program does, so each of those is refused with the reason.
//! The two argument diagnostics are GCC's own words.

cinrs::gnu99! {
    static void f(int *p) { (void)p; }
    static void wrong(long *p) { (void)p; }
    static void two(int *p, int *q) { (void)p; (void)q; }
    static int not_a_function;

    int at_file_scope __attribute__((cleanup(f))); //~ ERROR: 'cleanup' attribute ignored on an object at file scope

    typedef int Cleaned __attribute__((cleanup(f))); //~ ERROR: 'cleanup' attribute ignored on a 'typedef'

    void on_a_parameter(int p __attribute__((cleanup(f)))) { (void)p; } //~ ERROR: 'cleanup' attribute ignored on a parameter

    void a_function(void) __attribute__((cleanup(f))); //~ ERROR: 'cleanup' attribute ignored on a function

    void storage_classes(void) {
        static int kept __attribute__((cleanup(f))); //~ ERROR: 'cleanup' attribute ignored on an object with static storage duration
        extern int elsewhere __attribute__((cleanup(f))); //~ ERROR: 'cleanup' attribute ignored on an 'extern' declaration
        static _Thread_local int per_thread __attribute__((cleanup(f))); //~ ERROR: 'cleanup' attribute ignored on a thread-local object
        (void)kept;
        (void)elsewhere;
        (void)per_thread;
    }

    void the_argument(void) {
        int a __attribute__((cleanup("f"))) = 0; //~ ERROR: cleanup argument not an identifier
        int b __attribute__((cleanup(not_a_function))) = 0; //~ ERROR: cleanup argument not a function
        int c __attribute__((cleanup(nowhere))) = 0; //~ ERROR: cleanup argument not a function
        int d __attribute__((cleanup(two))) = 0; //~ ERROR: the cleanup function is called with one argument
        int e __attribute__((cleanup(wrong))) = 0; //~ ERROR: passing 'int *' to parameter 1 of 'wrong'
        (void)a; (void)b; (void)c; (void)d; (void)e;
    }
}

fn main() {}
