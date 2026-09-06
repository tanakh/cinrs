//! The three bundled POSIX headers say so when the target is Windows.
//!
//! The Microsoft C runtime has no header of any of these names and exports
//! none of their functions, so declaring them for a Windows target would turn
//! a compile error into a link error nobody can read. The `#error` names the
//! nearest equivalent instead.

cinrs::c99! {
    #pragma cinrs target "x86_64-pc-windows-msvc"
    #include <unistd.h> //~ ERROR: <unistd.h> is a POSIX header
    #include <fcntl.h> //~ ERROR: This <fcntl.h> is the POSIX one
    #include <strings.h> //~ ERROR: <strings.h> is a POSIX header
}

fn main() {}
