/* A header that reports how deep it is, for `__INCLUDE_LEVEL__`. */
#ifndef CINRS_INCLUDE_LEVEL_H
#define CINRS_INCLUDE_LEVEL_H

static int header_level(void) { return __INCLUDE_LEVEL__; }

#endif /* CINRS_INCLUDE_LEVEL_H */
