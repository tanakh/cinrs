//@check-pass
//! A C23 program the front end accepts end to end: the keywords, attributes,
//! the preprocessor additions and the new literal forms together.

cinrs::c23! {
    #include <stddef.h>
    #include <stdio.h>

    #define LOG(fmt, ...) snprintf(buf, size, fmt __VA_OPT__(,) __VA_ARGS__)

    constexpr int LIMIT = 0b100;

    static_assert(LIMIT == 4, "binary constants are C23");
    static_assert(sizeof(bool) == 1);

    enum Level : unsigned char { LOW, HIGH };

    struct Reading {
        int id;
        union {
            int as_int;
            double as_double;
        };
    };

    [[nodiscard]] bool is_high([[maybe_unused]] enum Level level) {
        return level == HIGH;
    }

    int describe(char *buf, unsigned long size, struct Reading r) {
        auto id = r.id;
        typeof(id) doubled = id * 2;
        switch (r.id) {
            case 0:
                [[fallthrough]];
            case LIMIT:
                return LOG("%d", doubled);
            default:
                return LOG("none");
        }
    }

    void *empty(void) {
        struct Reading r = {};
        if (r.id == 0) return nullptr;
        unreachable();
    }

    #ifdef NOT_DEFINED
    int unreachable_branch(void);
    #elifndef NOT_DEFINED
    int taken_branch(void);
    #endif
}

fn main() {}
