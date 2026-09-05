//! What the declaration list of an old-style definition may not say.
//!
//! C99 6.9.1p6: every declaration in it declares a parameter from the
//! identifier list, once, with no initialiser; `register` is the one storage
//! class a parameter may have. The identifier list itself is only legal in a
//! *definition* (6.7.5.3p3).

cinrs::c99! {
    int stray(a)
        int a;
        int b;      //~ ERROR: declaration for parameter 'b', which is not in the identifier list
    {
        return a;
    }

    int twice_over(a)
        int a;
        long a;     //~ ERROR: redefinition of parameter 'a'
    {
        return a;
    }

    int initialised(a)
        int a = 1;  //~ ERROR: parameter 'a' cannot have an initializer
    {
        return a;
    }

    int stored(a)
        static int a;   //~ ERROR: 'static' is not allowed on a parameter
    {
        return a;
    }

    int declared(a, b); //~ ERROR: an identifier list is only allowed in a function definition
}

fn main() {}
