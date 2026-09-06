//! `__attribute__((mode(M)))` names a *width*, and the declaration says the
//! rest. The modes that name a type this crate does not have are refused with
//! the reason rather than rounded to something else.

cinrs::gnu99! {
    /* The ones that work: the width comes from the mode and the signedness
     * from the type that was written. */
    typedef unsigned int u8 __attribute__((mode(QI)));
    typedef int s16 __attribute__((mode(__HI__)));
    typedef int word __attribute__((mode(word)));
    unsigned long widths(void) {
        return sizeof(u8) + sizeof(s16) * 10 + sizeof(word) * 100;
    }

    /* An extended or a quad floating format. */
    typedef double extended __attribute__((mode(XF))); //~ ERROR: no stable Rust type

    /* `SC` and `DC` are `float _Complex` and `double _Complex`, which do
     * exist; the wider complex modes name a format that does not. */
    typedef double complex_double __attribute__((mode(DC)));
    typedef double complex_quad __attribute__((mode(TC))); //~ ERROR: names a complex type

    /* A vector mode. */
    typedef int four_ints __attribute__((mode(V4SI))); //~ ERROR: names a vector type

    /* A mode nobody has. */
    typedef int nonsense __attribute__((mode(ZZ))); //~ ERROR: unknown machine mode

    /* `mode` says how wide an arithmetic type is, and a struct is not one. */
    struct s { int a; };
    typedef struct s wide_struct __attribute__((mode(DI))); //~ ERROR: applies to an arithmetic type
}

fn main() {}
