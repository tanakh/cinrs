/* A header beside the `.c` file that includes it — which is what
 * `#include "geometry.h"` finds first, since a quoted include searches the
 * directory of the file the directive is written in. */
#ifndef CINRS_TEST_GEOMETRY_H
#define CINRS_TEST_GEOMETRY_H

#define ORIGIN_X 0
#define ORIGIN_Y 0

struct Point {
    int x;
    int y;
};

int point_manhattan(struct Point p);
struct Point point_translate(struct Point p, int dx, int dy);

#endif /* CINRS_TEST_GEOMETRY_H */
