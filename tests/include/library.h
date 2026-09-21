/* A header in the shape a C library ships: a type, a macro and the
 * declarations of functions something else defines. `tests/declared_names.rs`
 * includes it twice over — once in the unit that defines them and once in the
 * unit that only declares them, which is the unit Rust calls through. */
#ifndef CINRS_TEST_LIBRARY_H
#define CINRS_TEST_LIBRARY_H

#define CINRS_TEST_LIBRARY_SCALE 3

struct Tally {
    int count;
    int total;
};

int cinrs_test_tally_add(struct Tally *t, int n);
int cinrs_test_tally_mean(struct Tally t);

/* A name Rust spells as a raw identifier, declared here so that both units
 * meet it through the header. */
int yield(int n);

#endif /* CINRS_TEST_LIBRARY_H */
