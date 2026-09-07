/* Packet headers as bit-fields: parse a buffer, rewrite the fields, pack it
 * back.
 *
 * This is the kernel most likely to be slow. A bit-field has no address, so it
 * is not a field of the generated Rust `struct` at all: a run of them shares
 * one `pub __cinrs_bitsN: [u8; K]` and each named member becomes a pair of
 * inherent methods, `p.ttl()` and `p.set_ttl(v)`. Every read and every write in
 * the C below is therefore a call — one that has to be inlined and folded back
 * into a shift-and-mask before this can keep up with a compiler that lays the
 * fields out itself.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

struct Header {
    unsigned version : 4;
    unsigned ihl : 4;
    unsigned dscp : 6;
    unsigned ecn : 2;
    unsigned total_length : 16;
    unsigned id : 16;
    unsigned flags : 3;
    unsigned frag_offset : 13;
    unsigned ttl : 8;
    unsigned protocol : 8;
    unsigned checksum : 16;
};

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 200000L;
    int reps = (argc > 2) ? atoi(argv[2]) : 200;
    struct Header *pkts = malloc((size_t) n * sizeof(struct Header));
    unsigned state = 606u;
    long i, live = 0, bytes = 0, sum = 0;
    int r;

    if (!pkts) {
        fprintf(stderr, "bitfields: out of memory\n");
        return 1;
    }
    for (i = 0; i < n; i++) {
        state = state * 1664525u + 1013904223u;
        pkts[i].version = 4;
        pkts[i].ihl = 5;
        pkts[i].dscp = (state >> 3) & 0x3fu;
        pkts[i].ecn = (state >> 9) & 3u;
        pkts[i].total_length = (state >> 11) & 0xffffu;
        pkts[i].id = (state >> 5) & 0xffffu;
        pkts[i].flags = (state >> 21) & 7u;
        pkts[i].frag_offset = (state >> 7) & 0x1fffu;
        pkts[i].ttl = 1 + ((state >> 13) & 0x7fu);
        pkts[i].protocol = (state >> 17) & 0xffu;
        pkts[i].checksum = 0;
    }

    for (r = 0; r < reps; r++) {
        live = 0;
        bytes = 0;
        sum = 0;
        for (i = 0; i < n; i++) {
            struct Header *p = &pkts[i];
            unsigned sumf;
            if (p->ttl == 0) continue;
            p->ttl = p->ttl - 1;
            if (p->ttl == 0) continue;
            live++;
            bytes += p->total_length;
            /* a stand-in for the header checksum: every field read once */
            sumf = p->version + p->ihl + p->dscp + p->ecn + p->total_length + p->id + p->flags +
                   p->frag_offset + p->ttl + p->protocol;
            p->checksum = (~sumf) & 0xffffu;
            sum += p->checksum;
            if (p->ttl < 4) p->ttl = 128;
        }
    }

    printf("bitfields n=%ld reps=%d live=%ld bytes=%ld sum=%ld size=%d\n", n, reps, live, bytes,
           sum, (int) sizeof(struct Header));
    free(pkts);
    return 0;
}
