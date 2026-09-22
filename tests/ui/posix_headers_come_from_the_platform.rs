//! A POSIX header with the platform's own directories switched off.
//!
//! The bundled set is ISO C; POSIX comes from the platform. So `<unistd.h>` is
//! not there — and up to 0.1.0 an incomplete copy of it *was*, which is why the
//! diagnostic says what to write rather than only which directories were
//! searched. A program that leaves out one line should be told which line.
//!
//! The last two show where the hint stops: a header that is nobody's — POSIX's
//! or otherwise — gets the plain message, and so does one asked for with
//! `#include "…"`, whose quoted form searched more places.

cinrs::c99! {
    #include <unistd.h> //~ ERROR: POSIX headers such as <unistd.h> come from the platform
    #include <sys/stat.h> //~ ERROR: '#pragma cinrs system_include'
    #include <pthread.h> //~ ERROR: CINRS_SYSTEM_INCLUDE=1
    #include <nowhere.h> //~ ERROR: <nowhere.h> file not found; searched: <cinrs>
    #include "unistd.h" //~ ERROR: POSIX headers such as <unistd.h> come from the platform
}

fn main() {}
