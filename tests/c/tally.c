/* A second file, compiled by `include_gnu99!`: the GNU entry points have an
 * `include_…!` of their own, so a file written for `gcc -std=gnu99` — a
 * statement expression, `typeof`, a case range — goes in as it stands. */

#define MAX(a, b) ({ __typeof__(a) _a = (a); __typeof__(b) _b = (b); _a > _b ? _a : _b; })

int tally_max(int a, int b)
{
    return MAX(a, b);
}

int tally_classify(int c)
{
    switch (c) {
    case '0' ... '9':
        return 1;
    case 'a' ... 'z':
        return 2;
    default:
        return 0;
    }
}
