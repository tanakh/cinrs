//! `#pragma GCC poison` and `#pragma pack`, which are the two GNU pragmas that
//! can be got wrong.

cinrs::c99! {
    #pragma GCC poison gets sprintf

    char *unsafe_copy(char *dst, const char *src) {
        return gets(dst); //~ ERROR: poisoned identifier
        //~^ ERROR: implicit declaration of function 'gets'
    }

    #pragma pack(3) //~ ERROR: #pragma pack expects
    #pragma pack(push 4) //~ ERROR: #pragma pack expects
    #pragma pack(pop) //~ ERROR: #pragma pack(pop) with nothing pushed
}

fn main() {}
