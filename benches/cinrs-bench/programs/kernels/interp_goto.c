/* The bytecode machine of `interp_switch.c`, dispatched with GCC's computed
 * `goto` — labels as values, `&&label` and `goto *p`.
 *
 * This needs `gnu11!`. In `cinrs` a function that jumps is lowered into a
 * state machine over basic blocks, and a computed `goto` falls out of that
 * machine as a `match` on the block index — so the two kernels are expected to
 * end up close to each other here, while natively the computed goto is
 * traditionally the faster of the two because each instruction gets its own
 * indirect branch to predict.
 *
 * It prints the same line as `interp_switch.c` for the same argument.
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

static int program[] = {
    OP_LOADI, 0, 0, 0,   /*  0: r0 = 0        (accumulator) */
    OP_LOADI, 1, 0, 0,   /*  1: r1 = iters    (patched)     */
    OP_LOADI, 2, 3, 0,   /*  2: r2 = 3                      */
    OP_LOADI, 6, 0, 0,   /*  3: r6 = 0                      */
    OP_MUL,   0, 0, 2,   /*  4: r0 = r0 * r2                */
    OP_ADD,   0, 0, 1,   /*  5: r0 = r0 + r1                */
    OP_SHR,   4, 0, 7,   /*  6: r4 = r0 >> 7                */
    OP_XOR,   0, 0, 4,   /*  7: r0 = r0 ^ r4                */
    OP_NEG,   6, 0, 0,   /*  8: r6 = -r6                    */
    OP_ADDI,  1, 1, -1,  /*  9: r1 = r1 - 1                 */
    OP_JNZ,   1, 4, 0,   /* 10: if r1 goto 4                */
    OP_HALT,  0, 0, 0    /* 11:                             */
};

int main(int argc, char **argv) {
    long iters = (argc > 1) ? atol(argv[1]) : 60000000L;
    /* Unsigned, for the reason `interp_switch.c` gives. */
    unsigned long r[8];
    long pc = 0;
    int i, x, y, z;

    /* Automatic rather than `static`: a label address is a run-time value in
     * the state machine `cinrs` lowers a jumping function into, so it belongs
     * in an initialiser that runs. */
    void *dispatch[] = {&&L_HALT, &&L_LOADI, &&L_ADD, &&L_SUB, &&L_MUL,  &&L_XOR,
                        &&L_SHR,  &&L_ADDI,  &&L_JNZ, &&L_JMP, &&L_NEG,  &&L_NOP};

    for (i = 0; i < 8; i++) r[i] = 0;
    program[1 * 4 + 2] = (int) (iters & 0x7fffffff);

#define NEXT()                       \
    do {                             \
        x = program[pc + 1];          \
        y = program[pc + 2];          \
        z = program[pc + 3];          \
        pc += 4;                      \
        goto *dispatch[program[pc - 4]]; \
    } while (0)

    NEXT();

L_LOADI:
    r[x] = (unsigned long) y;
    NEXT();
L_ADD:
    r[x] = r[y] + r[z];
    NEXT();
L_SUB:
    r[x] = r[y] - r[z];
    NEXT();
L_MUL:
    r[x] = r[y] * r[z];
    NEXT();
L_XOR:
    r[x] = r[y] ^ r[z];
    NEXT();
L_SHR:
    r[x] = r[y] >> z;
    NEXT();
L_ADDI:
    r[x] = r[y] + (unsigned long) (long) z;
    NEXT();
L_JNZ:
    if (r[x]) pc = (long) y * 4;
    NEXT();
L_JMP:
    pc = (long) x * 4;
    NEXT();
L_NEG:
    r[x] = -r[x];
    NEXT();
L_NOP:
    NEXT();
L_HALT:
    printf("interp iters=%ld acc=%lu\n", iters, r[0]);
    return 0;
}
