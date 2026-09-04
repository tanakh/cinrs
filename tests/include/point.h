/* A user header with the classic include guard, a type, a macro and the
 * declarations of functions another header defines. */
#ifndef POINT_H
#define POINT_H

#include "vec.h"

#define POINT_ORIGIN_X 0
#define POINT_ORIGIN_Y 0

struct Point {
    int x;
    int y;
};

int point_manhattan(struct Point p);
struct Point point_translate(struct Point p, int dx, int dy);

#endif /* POINT_H */
