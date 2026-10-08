//! Integration tests that *run* translated C reading and writing `volatile`
//! objects.
//!
//! What `volatile` changes — that every access happens, once, in order — is
//! invisible to a program that only looks at values, which is why the
//! expansion's shape is snapshotted in `cinrs-core`'s codegen tests and the
//! one observable consequence, a flag a signal handler sets, is run by
//! `ccinrs`'s. What is checked here is that every form of access computes what
//! C says it does: the volatile read and write paths are paths of their own.

use cinrs::{c99, gnu99};

#[test]
fn every_form_of_volatile_access_computes_what_c_says() {
    gnu99! {
        struct regs { int ctrl; volatile int status; unsigned mode : 3, irq : 1; };
        struct pk { char c; volatile int v; } __attribute__((packed));
        typedef volatile int vint;

        volatile int counter;
        volatile int table[4];
        volatile struct regs dev;
        struct regs plain;
        struct pk packed;
        vint through_typedef;

        int forms(volatile int *p, struct regs *r, volatile struct regs *vr, int i) {
            volatile int local = 1;
            int sum = 0;
            counter = 1;
            counter = 2;
            counter += 3;
            sum += counter++;
            sum += ++counter;
            table[1] = 1;
            table[1] = 2;
            table[i & 3] += 5;
            sum += table[i & 3];
            *p = 7;
            *p = 8;
            sum += *p;
            r->status = 9;
            sum += r->ctrl;
            vr->ctrl = 10;
            vr->mode = 5;
            vr->irq++;
            sum += vr->mode;
            dev.ctrl = 11;
            dev = plain;
            plain = dev;
            packed.v = 12;
            sum += packed.v;
            through_typedef = 13;
            local = local + 1;
            sum += local;
            sum += *(volatile int *)&plain.ctrl;
            int x = (counter = 4);
            return sum + x + (vr->irq = 3) + (vr->mode += 7);
        }

        int observed(int which) {
            switch (which) {
            case 0: return counter;
            case 1: return table[1];
            case 2: return table[2];
            case 3: return dev.mode;
            case 4: return dev.ctrl;
            default: return through_typedef + packed.v;
            }
        }
    }

    let mut v: core::ffi::c_int = 0;
    // The sum, worked through by hand (and what GCC prints): `counter++` is 5
    // and `++counter` 7; table[2] is 5; *p is 8; plain.ctrl is 0; vr->mode
    // is 5; packed.v is 12; local is 2; the cast reads plain.ctrl, 0; the
    // assignment's value is 4. Then `dev = plain` has cleared dev's bits, so
    // `vr->irq = 3` is 1, truncated to its one bit, and `vr->mode += 7` is 7.
    let sum = unsafe { forms(&raw mut v, &raw mut plain, &raw mut dev, 6) };
    assert_eq!(sum, 5 + 7 + 5 + 8 + 5 + 12 + 2 + 4 + 1 + 7);
    assert_eq!(v, 8);
    unsafe {
        assert_eq!(observed(0), 4);
        assert_eq!(observed(1), 2);
        assert_eq!(observed(2), 5);
        assert_eq!(observed(3), 7);
        assert_eq!(observed(4), 0);
        assert_eq!(observed(5), 13 + 12);
    }
}

/// A `volatile` local a nested function captures is reached through a pointer
/// to `volatile`, and stays one there.
#[test]
fn a_captured_volatile_local_stays_volatile() {
    gnu99! {
        int triangle(int n) {
            volatile int total = 0;
            void add(int k) { total += k; }
            for (int i = 1; i <= n; i++)
                add(i);
            return total;
        }
    }

    assert_eq!(unsafe { triangle(10) }, 55);
}

/// A `volatile` local of a safe function is accessed in an `unsafe` block of
/// its own — its address is valid by construction — so the function stays
/// safe.
#[test]
fn a_safe_function_may_have_a_volatile_local() {
    c99! {
        #pragma cinrs safe count_down

        int count_down(int n) {
            volatile int left = n;
            int steps = 0;
            while (left > 0) {
                left--;
                steps++;
            }
            return steps + left;
        }
    }

    assert_eq!(count_down(5), 5);
    assert_eq!(count_down(-3), -3);
}
