/* The SQLite amalgamation as one cinrs translation unit, with
 * `SQLITE_THREADSAFE 0`.
 *
 * The same wrapper as `sqlite_threadsafe.c` with the threading mode switched
 * off, which takes every mutex out of the build. It is a separate file rather
 * than a `-D` because both the pragma and the macro have to be in the unit's
 * own text.
 *
 * The pragma is `system_include first` here, and the reason is worth writing
 * down. With threads off, SQLite's `unixSleep` is the only caller of
 * `nanosleep` — a POSIX function that lives in the ISO C header `<time.h>`. In
 * the plain mode the *bundled* `<time.h>` wins, and it declares the ISO C part
 * only, so `nanosleep` is not declared and the call is an error. With threads
 * on the same program compiles in the plain mode, because `<pthread.h>` is one
 * of the platform's headers and reaches the platform's `<time.h>` for itself.
 * `first` takes every header from the platform and so has all of POSIX. See
 * `doc/system-headers.md`, "The switch".
 */
#pragma cinrs system_include first
#define SQLITE_THREADSAFE 0
#include "../../../target/sqlite/sqlite3.c"
