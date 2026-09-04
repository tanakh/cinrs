//! Calling something that is not a function, taking the address of something
//! that is not an object, and assigning a string literal to an `int`.

cinrs::c99! {
    int call_an_object(int n) {
        return n(1); //~ ERROR: called object 'n' is not a function
    }

    int address_of_a_value(int n) {
        int *p = &(n + 1); //~ ERROR: cannot take the address of an rvalue of type 'int'
        return *p;
    }

    int string_to_int(void) {
        int n = "text"; //~ ERROR: cannot initialize 'n', of type 'int', with an expression of type 'char *'
        return n;
    }
}

fn main() {}
