//! A C23 keyword is an ordinary identifier in a `c11!` block — `<stdbool.h>`
//! depends on it — so the gate is reported where the name is used.

cinrs::c11! {
    void *nothing(void) {
        return nullptr; //~ ERROR: requires C23 or later
    }

    constexpr int LIMIT = 4; //~ ERROR: requires C23 or later
}

fn main() {}
