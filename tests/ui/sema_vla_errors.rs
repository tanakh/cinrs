//! Where a variably modified type may not be declared, and what may not be
//! jumped over.
//!
//! Variable length arrays and the types built on them — `int a[n][m]`,
//! `int (*p)[n]`, `typedef int T[n]`, and the parameter forms — work at block
//! scope. What is left here is what C itself forbids: a size nobody could
//! evaluate, a member whose record would have no layout, and a jump past the
//! declaration that computes the bound.

cinrs::c99! {
    int n;
    /* At file scope there is no moment at which the bound could be evaluated,
       so an array there needs a constant one as it always did. */
    int at_file_scope[n]; //~ ERROR: array size is not an integer constant expression
    int two_dimensions[n][n]; //~ ERROR: array size is not an integer constant expression
    /* A `typedef` of one is refused where the type is *used*, which is the
       first place the missing bound could matter. */
    typedef int Row[n];
    Row a_row; //~ ERROR: array size is not an integer constant expression

    void a_member(int len) {
        struct Buffer {
            int len;
            int data[len]; //~ ERROR: a member of a struct cannot have a variably modified type
        };
        union Either {
            int one;
            char many[len][2]; //~ ERROR: a member of a union cannot have a variably modified type
        };
    }

    void storage_classes(int len) {
        static int kept[len]; //~ ERROR: a variable length array cannot have static storage duration
        extern int elsewhere[len]; //~ ERROR: a variable length array cannot have static storage duration
        static int (*row)[len]; //~ ERROR: a variably modified type cannot have static storage duration
        (void)kept;
        (void)row;
    }

    void with_an_initializer(int len) {
        int values[len] = { 1, 2, 3 }; //~ ERROR: a variable length array cannot have an initializer
        int square[len][2] = { { 1, 2 } }; //~ ERROR: a variable length array cannot have an initializer
        (void)values; (void)square;
    }

    void alignment(int len) {
        _Alignas(16) int values[len];
        //~^ ERROR: '_Alignas' requires C11 or later
        //~| ERROR: an alignment specifier is not supported on a variable length array
        (void)values;
    }

    void a_bound_that_is_not_in_scope(int len) {
        int square[len][cols]; //~ ERROR: use of undeclared identifier 'cols'
        (void)square;
    }

    void a_star_outside_a_prototype(int len) {
        int values[*]; //~ ERROR: '[*]' is only allowed in a function prototype
        (void)values;
        (void)len;
    }

    int jump_into_the_scope(int len) {
        goto past; //~ ERROR: jump into the scope of an identifier with variably modified type
        int values[len];
        values[0] = 1;
    past:
        return len;
    }

    int jump_into_a_two_dimensional_scope(int len) {
        goto past; //~ ERROR: jump into the scope of an identifier with variably modified type
        int values[len][len];
        values[0][0] = 1;
    past:
        return len;
    }

    int jump_into_the_scope_of_a_pointer(int len) {
        goto past; //~ ERROR: jump into the scope of an identifier with variably modified type
        int (*row)[len];
        row = 0;
    past:
        return len;
    }

    int case_into_the_scope(int len, int which) {
        switch (which) {
        case 0: {
            int values[len];
            values[0] = 1;
        case 1: //~ ERROR: jump into the scope of an identifier with variably modified type
            return values[0];
        }
        }
        return 0;
    }

    int in_a_switch_body(int len, int which) {
        switch (which) {
        int values[len]; //~ ERROR: a variable length array cannot be declared directly in the body of a 'switch'
        case 0:
            return values[0];
        }
        return 0;
    }

    /* `sizeof` of one is a value, not a constant, so none of the three places
       C needs a constant expression will take it. */
    int a_constant_expression(int len) {
        int values[len];
        static int table[sizeof values]; //~ ERROR: a variable length array cannot have static storage duration
        static unsigned long size = sizeof values; //~ ERROR: initializer is not a compile-time constant expression
        switch (len) {
        case sizeof values: //~ ERROR: 'case' label is not a compile-time constant expression
            return 1;
        }
        return table[0] + (int)size;
    }
}

fn main() {}
