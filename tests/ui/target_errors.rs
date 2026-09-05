//! What goes wrong when a unit is told which machine to translate for.
//!
//! `#pragma cinrs target` picks the data model, overriding `CINRS_TARGET`; a
//! triple naming a machine cinrs has no model for is refused rather than
//! guessed at, and so are the two constructs a real target refuses — GNU's
//! `__int128` on a 32-bit architecture, which is what GCC does, and a
//! bit-field on a big-endian one, which cinrs does not lay out.
//!
//! The `CINRS_TARGET` half of the same diagnostic — the one that says to unset
//! the variable rather than to change the pragma — is checked by
//! `tests/cross_targets.rs`, which can set an environment variable for one
//! compilation without disturbing the rest of the suite.

/* A triple whose architecture is not in the table. */
cinrs::c11! {
    #pragma cinrs target "pdp11-unknown-unix"
    //~^ ERROR: unknown architecture 'pdp11'
    int x;
}

/* A triple whose operating system is not in the table. */
cinrs::c11! {
    #pragma cinrs target "x86_64-unknown-plan9"
    //~^ ERROR: unsupported operating system 'plan9'
    int y;
}

/* An architecture whose data model cinrs does not implement at all. */
cinrs::c11! {
    #pragma cinrs target "avr-none-unknown"
    //~^ ERROR: on 'avr' 'int' is 16 bits and 'double' is 32
    int z;
}

/* The pragma takes exactly one string literal. */
cinrs::c11! {
    #pragma cinrs target
    //~^ ERROR: #pragma cinrs target needs a string literal
    int a;
}

cinrs::c11! {
    #pragma cinrs target 42
    //~^ ERROR: #pragma cinrs target needs a string literal, found integer constant
    int b;
}

cinrs::c11! {
    #pragma cinrs target ""
    //~^ ERROR: #pragma cinrs target was given an empty string
    int c;
}

/* Two of them have to agree: the model is chosen once, for the whole unit. */
cinrs::c11! {
    #pragma cinrs target "i686-unknown-linux-gnu"
    #pragma cinrs target "wasm32-unknown-unknown"
    //~^ ERROR: already translated for 'i686-unknown-linux-gnu'
    int d;
}

/* And it has to come before anything the old model already answered. */
cinrs::c11! {
    #include <limits.h>
    #pragma cinrs target "i686-unknown-linux-gnu"
    //~^ ERROR: must come before every '#include' and '#if'
    int e;
}

/* GCC has `__int128` on the 64-bit architectures only, and refuses it rather
 * than emulating it on a 32-bit one. */
cinrs::gnu11! {
    #pragma cinrs target "i686-unknown-linux-gnu"
    __int128 wide(void) { return 0; }
    //~^ ERROR: '__int128' is not available on this target (x86, 32-bit pointers)

    unsigned __int128 uwide(void) { return 0; }
    //~^ ERROR: 'unsigned __int128' is not available on this target
}

/* Where a bit-field's bits sit inside its storage unit is implementation
 * defined, and cinrs allocates from the least significant end — which is not
 * how a big-endian ABI does it. */
cinrs::c11! {
    #pragma cinrs target "s390x-unknown-linux-gnu"
    struct flags {
        unsigned a : 3;
        //~^ ERROR: bit-field 'a' is not supported on a big-endian target (s390x)
        unsigned b : 5;
        //~^ ERROR: bit-field 'b' is not supported on a big-endian target (s390x)
    };
}

fn main() {}
