/* A header with a mistake in it, for the test of how one is reported. */
#ifndef BAD_H
#define BAD_H

int broken(void) {
    return undeclared_thing;
}

#endif /* BAD_H */
