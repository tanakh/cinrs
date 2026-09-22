//! The atomic types and operations that are refused rather than mistranslated.
//!
//! Two kinds of refusal are here. C's own constraints — `_Atomic` on an array
//! or a function type, arithmetic on a function pointer — and the ones this
//! crate adds, every one of them a type `core::sync::atomic` has nothing for:
//! an aggregate, a 128-bit integer. An atomic `struct` is the one that is
//! perfectly good C: it needs a lock, and there is nothing in the generated
//! Rust to be one.

cinrs::c11! {
    struct pair { int a, b; };
    typedef int array[4];
    typedef int function(void);

    _Atomic struct pair record;
    //~^ ERROR: '_Atomic struct pair' is not supported yet

    _Atomic array flat; //~ ERROR: may not be applied to the array type 'int[4]'

    _Atomic function fun; //~ ERROR: may not be applied to the function type 'int (void)'

    /* A function pointer is fine: an `Option<fn>` is pointer-sized with a
       null niche, so it goes through `AtomicPtr` like any other pointer. */
    _Atomic(int (*)(void)) callback;

    _Atomic __int128 wide; //~ ERROR: there is no stable 128-bit atomic

    _Atomic void nothing; //~ ERROR: '_Atomic void' is not a type

    struct opaque;
    _Atomic(struct opaque) incomplete; //~ ERROR: requires a complete type

    struct with_bits { _Atomic int b : 3; };
    //~^ ERROR: bit-field 'b' has invalid type '_Atomic(int)'

    /* Packing can leave the member somewhere no atomic instruction can reach:
       GCC answers such a member with a `libatomic` call that takes a lock,
       and there is nothing here that could. */
    struct __attribute__((packed)) squeezed { char a; _Atomic int b; };
    //~^ ERROR: packing puts the '_Atomic' member 'b' at offset 1

    /* A one-byte atomic in the same record is fine: nothing moved it. */
    struct __attribute__((packed)) roomy { char a; _Atomic char b; };

    /* The builtins say the same thing about the same types. */
    __int128 wide_object;
    int wide_atomic(void) {
        return (int) __atomic_load_n(&wide_object, __ATOMIC_SEQ_CST);
        //~^ ERROR: '__atomic_load_n' cannot operate on '__int128'
    }

    float value;
    float float_add(void) {
        return __atomic_fetch_add(&value, 1.0f, __ATOMIC_SEQ_CST);
        //~^ ERROR: '__atomic_fetch_add' does not work on 'float'
    }

    int pointer_or(int **p) {
        return __atomic_fetch_or(p, 1, __ATOMIC_SEQ_CST) != 0;
        //~^ ERROR: does not work on a pointer object: only '+' and '-' do
    }

    /* Loading and storing one is allowed; there is no arithmetic on a
       function pointer in C, so there is none here either. */
    int (*hook)(void);
    int hook_add(void) {
        return __atomic_fetch_add(&hook, 1, __ATOMIC_SEQ_CST) != 0;
        //~^ ERROR: there is no arithmetic on a function pointer
    }

    int bool_add(_Atomic _Bool *p) {
        return __atomic_fetch_add(p, 1, __ATOMIC_SEQ_CST);
        //~^ ERROR: does not work on a '_Bool' object
    }

    int wide_test_and_set(int *p) {
        return __atomic_test_and_set(p, __ATOMIC_SEQ_CST);
        //~^ ERROR: needs a pointer to a one-byte object
    }

    int not_a_pointer(int n) {
        return __atomic_load_n(n, __ATOMIC_SEQ_CST);
        //~^ ERROR: the first argument of '__atomic_load_n' must be a pointer
    }

    int wrong_arity(int *p) {
        return __atomic_load_n(p); //~ ERROR: '__atomic_load_n' expects 2 arguments, have 1
    }
}

fn main() {}
