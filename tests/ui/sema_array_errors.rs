//! Array bounds must be constants, an array is not assignable, and a variable
//! length array is not supported yet.

cinrs::c99! {
    int length;
    int table[length]; //~ ERROR: array size is not an integer constant expression

    int variable_length(int n) {
        int values[n]; //~ ERROR: variable length arrays are not supported yet
        return values[0];
    }

    void assign_to_an_array(void) {
        int values[3];
        int other[3];
        values = other; //~ ERROR: array type 'int[3]' is not assignable
    }
}

fn main() {}
