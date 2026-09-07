/* An open-addressing hash table with linear probing: insert, then look up.
 *
 * A `struct` of two members held in a flat array, addressed by a hash and
 * probed forward. Random access over a table larger than the last-level cache,
 * so the loads miss; what the compiler contributes is the address arithmetic
 * and the loop over a probe sequence.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

typedef struct {
    unsigned long long key;
    long value;
} Slot;

#define EMPTY 0ull

static unsigned long long mix(unsigned long long x) {
    x ^= x >> 33;
    x *= 0xff51afd7ed558ccdull;
    x ^= x >> 33;
    x *= 0xc4ceb9fe1a85ec53ull;
    x ^= x >> 33;
    return x;
}

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 4000000L;
    long lookups = (argc > 2) ? atol(argv[2]) : 8000000L;
    long cap = 1;
    Slot *table;
    long i, probes = 0, found = 0, sum = 0;
    unsigned long long mask;

    while (cap < n * 2) cap <<= 1;
    mask = (unsigned long long) (cap - 1);
    table = calloc((size_t) cap, sizeof(Slot));
    if (!table) {
        fprintf(stderr, "hashtable: out of memory\n");
        return 1;
    }

    for (i = 0; i < n; i++) {
        unsigned long long key = (unsigned long long) i + 1;
        unsigned long long h = mix(key) & mask;
        while (table[h].key != EMPTY) {
            probes++;
            h = (h + 1) & mask;
        }
        table[h].key = key;
        table[h].value = i * 3;
    }

    for (i = 0; i < lookups; i++) {
        unsigned long long key = (unsigned long long) ((i * 2654435761ull) % (unsigned long long) (2 * n)) + 1;
        unsigned long long h = mix(key) & mask;
        while (table[h].key != EMPTY) {
            if (table[h].key == key) {
                found++;
                sum += table[h].value % 1000003;
                break;
            }
            probes++;
            h = (h + 1) & mask;
        }
    }

    printf("hashtable n=%ld lookups=%ld found=%ld probes=%ld sum=%ld\n", n, lookups, found, probes,
           sum);
    free(table);
    return 0;
}
