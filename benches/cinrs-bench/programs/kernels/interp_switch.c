/* A bytecode interpreter whose dispatch is a `switch`, with a deliberate
 * fallthrough.
 *
 * The register machine below is four `int`s wide per instruction and runs a
 * small loop many millions of times, so essentially all of the time is in the
 * dispatch: load the opcode, jump through the switch table, do a couple of
 * arithmetic operations, go round. C's `switch` becomes a Rust `match` over a
 * jump table, and a fallthrough case has to be lowered into something that
 * runs the next case's body without re-dispatching; this measures whether it
 * did.
 *
 * `interp_goto.c` is the same machine with GCC's computed `goto`, and prints
 * the same line.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

enum {
    OP_HALT = 0,
    OP_LOADI,
    OP_ADD,
    OP_SUB,
    OP_MUL,
    OP_XOR,
    OP_SHR,
    OP_ADDI,
    OP_JNZ,
    OP_JMP,
    OP_NEG,
    OP_NOP
};

/* op, x, y, z — four `int`s each, so the program counter moves by four. */
static int program[] = {
    OP_LOADI, 0, 0, 0,   /*  0: r0 = 0        (accumulator) */
    OP_LOADI, 1, 0, 0,   /*  1: r1 = iters    (patched)     */
    OP_LOADI, 2, 3, 0,   /*  2: r2 = 3                      */
    OP_LOADI, 6, 0, 0,   /*  3: r6 = 0                      */
    OP_MUL,   0, 0, 2,   /*  4: r0 = r0 * r2                */
    OP_ADD,   0, 0, 1,   /*  5: r0 = r0 + r1                */
    OP_SHR,   4, 0, 7,   /*  6: r4 = r0 >> 7                */
    OP_XOR,   0, 0, 4,   /*  7: r0 = r0 ^ r4                */
    OP_NEG,   6, 0, 0,   /*  8: r6 = -r6, then falls into NOP */
    OP_ADDI,  1, 1, -1,  /*  9: r1 = r1 - 1                 */
    OP_JNZ,   1, 4, 0,   /* 10: if r1 goto 4                */
    OP_HALT,  0, 0, 0    /* 11:                             */
};

int main(int argc, char **argv) {
    long iters = (argc > 1) ? atol(argv[1]) : 60000000L;
    /* Unsigned: the accumulator is meant to wrap, and signed overflow is
     * undefined — which gcc and clang may exploit and `cinrs` may not, so the
     * three builds would stop agreeing. */
    unsigned long r[8];
    long pc = 0;
    int i;

    for (i = 0; i < 8; i++) r[i] = 0;
    program[1 * 4 + 2] = (int) (iters & 0x7fffffff);

    for (;;) {
        int op = program[pc];
        int x = program[pc + 1];
        int y = program[pc + 2];
        int z = program[pc + 3];
        pc += 4;
        switch (op) {
            case OP_LOADI:
                r[x] = (unsigned long) y;
                break;
            case OP_ADD:
                r[x] = r[y] + r[z];
                break;
            case OP_SUB:
                r[x] = r[y] - r[z];
                break;
            case OP_MUL:
                r[x] = r[y] * r[z];
                break;
            case OP_XOR:
                r[x] = r[y] ^ r[z];
                break;
            case OP_SHR:
                r[x] = r[y] >> z;
                break;
            case OP_ADDI:
                r[x] = r[y] + (unsigned long) (long) z;
                break;
            case OP_JNZ:
                if (r[x]) pc = (long) y * 4;
                break;
            case OP_JMP:
                pc = (long) x * 4;
                break;
            case OP_NEG:
                r[x] = -r[x];
                /* fall through */
            case OP_NOP:
                break;
            case OP_HALT:
                goto done;
            default:
                fprintf(stderr, "interp: bad opcode %d\n", op);
                return 1;
        }
    }

done:
    printf("interp iters=%ld acc=%lu\n", iters, r[0]);
    return 0;
}
