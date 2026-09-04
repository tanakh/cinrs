//@check-pass
//! The GNU extensions a strict entry point accepts, and the portability idioms
//! that go with them: nothing here is reported, and the generated Rust trips
//! no lint either.

cinrs::c99! {
    /* Every portability header in the world writes this, and it has to keep
     * working: `__attribute__` is an ordinary identifier until the tokens
     * reach the parser, so a macro of that name defines and expands. */
    #if !defined(__GNUC__) || __GNUC__ < 4
    #define __attribute__(x)
    #endif

    #if __has_attribute(packed) && __has_builtin(__builtin_expect)
    #define HAVE_GNU 1
    #endif

    #define likely(x) __builtin_expect(!!(x), 1)
    #define max(a, b) ({ __typeof__(a) _a = (a); __typeof__(b) _b = (b); _a > _b ? _a : _b; })

    struct __attribute__((packed)) Wire {
        unsigned char kind;
        unsigned int length;
        unsigned short flags : 9;
    };

    struct Frame { unsigned int len; unsigned char data[]; };

    __attribute__((always_inline)) static int biggest(int a, int b) { return max(a, b); }

    __attribute__((constructor)) static void setup(void) { }

    int classify(int c) {
        __label__ done;
        int result;
        switch (c) {
        case '0' ... '9': result = 1; goto done;
        case 'a' ... 'z':
        case 'A' ... 'Z': result = 2; goto done;
        default: result = likely(c) ? 0 : -1; goto done;
        }
    done:
        return result;
    }

    static const signed char digits[128] = { ['0' ... '9'] = 1, ['a' ... 'f'] = 2 };

    unsigned long sizes(void) {
        return sizeof(struct Wire) + sizeof(struct Frame) + sizeof(digits)
             + sizeof(__func__) + (unsigned long) biggest(1, 2);
    }

    int elvis(int a, int b) { return a ?: b; }
}

fn main() {}
