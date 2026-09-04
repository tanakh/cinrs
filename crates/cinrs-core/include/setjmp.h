/* <setjmp.h> — not supported.
 *
 * `setjmp` and `longjmp` unwind by restoring a saved machine context, which
 * has no meaning in the Rust cinrs generates: the state a `longjmp` would jump
 * back into is Rust's, and the compiler is entitled to assume nothing leaves a
 * function that way. Saying so here beats letting a program compile and then
 * corrupt itself.
 */
#error setjmp/longjmp are not supported by cinrs
