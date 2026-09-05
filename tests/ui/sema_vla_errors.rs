//! Where a variable length array may not be declared, and what may not be
//! jumped over.
//!
//! A one-dimensional variable length array at block scope is supported; every
//! other variably modified type is refused, because the size would have to be
//! carried around by the *type* rather than by the object.

cinrs::c99! {
    int n;
    /* At file scope there is no moment at which the bound could be evaluated,
       so an array there needs a constant one as it always did. */
    int at_file_scope[n]; //~ ERROR: array size is not an integer constant expression

    void a_member(int len) {
        struct Buffer {
            int len;
            int data[len]; //~ ERROR: a member of a struct cannot have a variably modified type
        };
        union Either {
            int one;
            char many[len]; //~ ERROR: a member of a union cannot have a variably modified type
        };
    }

    void a_typedef(int len) {
        typedef int Row[len]; //~ ERROR: variably modified types other than a one-dimensional array
    }

    void storage_classes(int len) {
        static int kept[len]; //~ ERROR: a variable length array cannot have static storage duration
        extern int elsewhere[len]; //~ ERROR: a variable length array cannot have static storage duration
        (void)kept;
    }

    void with_an_initializer(int len) {
        int values[len] = { 1, 2, 3 }; //~ ERROR: a variable length array cannot have an initializer
        (void)values;
    }

    void more_than_one_dimension(int len) {
        int square[len][len]; //~ ERROR: variably modified types other than a one-dimensional array
        int mixed[3][len]; //~ ERROR: variably modified types other than a one-dimensional array
        int (*row)[len]; //~ ERROR: variably modified types other than a one-dimensional array
        (void)square; (void)mixed; (void)row;
    }

    int jump_into_the_scope(int len) {
        goto past; //~ ERROR: jump into the scope of an identifier with variably modified type
        int values[len];
        values[0] = 1;
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
