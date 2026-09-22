/* The SQLite amalgamation as one cinrs translation unit, in its default
 * configuration.
 *
 * This wrapper exists for one reason: `#pragma cinrs system_include` has to be
 * in the *unit's text*, and the amalgamation is not to be edited — it is
 * downloaded, verified against the hash sqlite.org publishes, and read as it
 * stands. So the unit is this file, which writes the pragma and includes it.
 *
 * The pragma is the plain form, not `first`: SQLite needs POSIX — `<unistd.h>`,
 * `<fcntl.h>`, `<sys/stat.h>`, `<sys/mman.h>`, `<pthread.h>` — which comes from
 * the platform, and everything ISO C keeps cinrs's own plain-C declarations. See
 * `doc/system-headers.md`.
 *
 * Nothing is defined here: `SQLITE_THREADSAFE` defaults to 1, which is the
 * serialized threading mode and the configuration the library ships in.
 */
#pragma cinrs system_include
#include "../../../target/sqlite/sqlite3.c"
