/* A header that renumbers itself with `#line`, so that a test can see the
 * numbering end with the header rather than leak into whatever included it. */
#ifndef CINRS_RENUMBERED_H
#define CINRS_RENUMBERED_H

#line 90 "elsewhere.c"
int renumbered_line(void) { return __LINE__; }
const char *renumbered_file(void) { return __FILE__; }

#endif /* CINRS_RENUMBERED_H */
