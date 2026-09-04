/* A nested header, guarded the other way round: `#pragma once`. Including it
 * twice — as point.h and the test itself both do — must define VEC_SCALE once
 * and read the file once. */
#pragma once

#define VEC_SCALE 3

static int vec_scaled(int v) {
    return v * VEC_SCALE;
}
