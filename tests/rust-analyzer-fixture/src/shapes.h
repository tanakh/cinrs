/* A user header, beside the `.rs` file that includes it: which is the whole
   point of it being here. A quoted `#include` searches the directory of the
   file the invocation is written in, and under a host that reports no positions
   that directory is only known because the invocation was found on disk. */
#ifndef SHAPES_H
#define SHAPES_H

struct Rect {
    int w;
    int h;
};

#endif
