//! Bit-fields: the widths and the types a member may not be given, and the
//! three things a member without an address of its own cannot be asked.

cinrs::c99! {
    struct Widths {
        int   too_wide  : 33;  //~ ERROR: exceeds the 32 bits of its type 'int'
        char  too_wide2 : 9;   //~ ERROR: exceeds the 8 bits of its type 'char'
        _Bool too_wide3 : 2;   //~ ERROR: exceeds the 1 bit of its type '_Bool'
        int   negative  : -1;  //~ ERROR: negative width in bit-field 'negative'
        int   nothing   : 0;   //~ ERROR: zero width for bit-field 'nothing'
    };

    struct Types {
        double  d : 3;         //~ ERROR: bit-field 'd' has invalid type 'double'
        int    *p : 3;         //~ ERROR: bit-field 'p' has invalid type 'int *'
        int    a[2] : 3;       //~ ERROR: bit-field 'a' has invalid type 'int[2]'
    };

    int width;
    struct NotConstant {
        int n : width;         //~ ERROR: is not an integer constant expression
    };

    struct Flags { unsigned int ready : 1; int level : 3; };

    unsigned long address_of(struct Flags *f) {
        return (unsigned long) &f->ready; //~ ERROR: cannot take the address of a bit-field
    }

    unsigned long size_of(struct Flags *f) {
        return sizeof f->level; //~ ERROR: 'sizeof' applied to a bit-field
    }

    unsigned long offset_of(void) {
        return __builtin_offsetof(struct Flags, level); //~ ERROR: 'offsetof' applied to the bit-field 'level'
    }
}

fn main() {}
