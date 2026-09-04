# Design note

This enables writing C code within Rust procedural macros.
Defined functions can be called directly from Rust.
Arguments and return values ​​correspond to C types defined in `core::ffi`, such as `c_int`.

```rust
c99!{
    // This defines `pub unsafe fn add(a: c_int, b: c_int) -> c_int;`
    int add(int a, int b) {
        return a + b;
    }
}
```

# Semantics

* The code actually generated will be Rust code resulting from a naive conversion, similar to `c2rs`.

* While there are plans to support various versions—such as `c99!`, `c11!`, `c17!`, and `c23!`—only `c99` will be implemented initially.

* Support for processing `#include <...>` directives will be included.
  * How to handle include paths needs to be determined.

* Each macro (e.g., `c99!`) will be treated as a single translation unit.

# Implementation detail

* In the event of a compilation error, I want the error to be reported—whether by the compiler or the IDE—at the specific location within the macro where the issue occurred.

* Ideally, I want to allow writing raw C code directly inside the macro; however, since C tokens that are invalid in Rust (such as `.0`) cannot be accepted directly, I will also support passing the code as string literals (or raw string literals) to handle such cases. Even in these instances, I want to ensure that error locations are accurately indicated in compiler error messages or within the IDE.
