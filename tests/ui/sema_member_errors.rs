//! Member access: a name the record does not have, and a base that is not a
//! record at all.

cinrs::c99! {
    struct Point { int x; int y; };

    int unknown_member(struct Point p) {
        return p.z; //~ ERROR: no member named 'z' in 'struct Point'
    }

    int not_a_record(int n) {
        return n.x; //~ ERROR: member reference base type 'int' is not a structure or union
    }

    int arrow_on_a_value(struct Point p) {
        return p->x; //~ ERROR: member reference type 'struct Point' is not a pointer; did you mean to use '.'?
    }
}

fn main() {}
