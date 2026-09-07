/* A translation unit in a file of its own, compiled by
 * `cinrs::include_c99!("c/geometry.c")` in `tests/include_c.rs`.
 *
 * Everything a `c99!` block may hold, this may hold: pragmas that configure
 * the unit, `#include` of a header beside it and of a bundled one, and any
 * number of definitions. */

#pragma cinrs module "geometry"
#pragma cinrs safe point_manhattan at_origin

#include "geometry.h"   /* found next to this file */
#include <string.h>     /* bundled, and linked against the real library */

int point_manhattan(struct Point p)
{
    return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
}

struct Point point_translate(struct Point p, int dx, int dy)
{
    struct Point out;
    out.x = p.x + dx;
    out.y = p.y + dy;
    return out;
}

int at_origin(struct Point p)
{
    return p.x == ORIGIN_X && p.y == ORIGIN_Y;
}

unsigned long label_length(const char *label)
{
    return (unsigned long) strlen(label);
}

/* `__FILE__` names this file rather than the `.rs` the macro is written in,
 * and `__LINE__` counts this file's own lines. */
const char *geometry_file(void)
{
    return __FILE__;
}

int geometry_line(void)
{
    return __LINE__;
}
