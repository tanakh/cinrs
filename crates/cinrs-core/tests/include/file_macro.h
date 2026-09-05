/* A header that includes itself by `__FILE__`.
 *
 * This is what Clang's own `clang/test/C/C99/Inputs/nested-include.h` does to
 * reach the fifteen levels of nested `#include` C23 5.2.5.2p1 asks for. It is
 * the awkward case for include resolution: `__FILE__` is the path the header
 * was found at, written relative to the working directory, and the directive
 * is inside *that* directory — so neither the including file's own directory
 * nor a `-I` will resolve it, and the working-directory step of
 * `include::resolve` is what does.
 */
#if __COUNTER__ < 3
#include __FILE__
#endif

level
