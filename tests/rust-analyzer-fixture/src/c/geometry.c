/* The file `include_c99!("c/geometry.c")` names: a path relative to the
   directory of the `.rs` file the invocation is written in, which a host that
   reports no positions only knows because the invocation was found on disk.

   Its own `#include "…"` is resolved beside *this* file, one directory up. */
#include "../scale.h"

int perimeter(int w, int h)
{
    return SCALE * 2 * (w + h);
}
