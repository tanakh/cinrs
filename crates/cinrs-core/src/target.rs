//! The target data model.
//!
//! C's arithmetic is defined in terms of the widths and the signedness of the
//! implementation's types: whether `-1 < 1u` holds depends on how wide `int`
//! is, `unsigned char a = 200, b = 100; a + b` is 300 rather than 44 only
//! because `int` is wider than `char`, and `'\xff'` is `-1` exactly when plain
//! `char` is signed. Sema therefore needs a concrete model of the machine
//! before it can type a single expression.
//!
//! # Known limitation
//!
//! [`TargetModel::host`] reads the configuration of the machine the procedural
//! macro itself was compiled for — that is, the *host*. When cross-compiling to
//! a target with a different data model (say a 64-bit Linux host building for
//! 64-bit Windows, where `long` is 32 bits) the model is wrong: integer
//! promotions and the typing of constants would follow the host's rules while
//! `::core::ffi::c_long` follows the target's. The generated code still uses
//! the `core::ffi` aliases, so simple programs are unaffected; programs whose
//! meaning depends on the width of `long` are not. Selecting the model through
//! a macro option is planned.

/// Widths (in bits) and `char` signedness of the machine the generated code
/// runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetModel {
    /// Whether plain `char` is a signed type. Signed on x86 and x86-64,
    /// unsigned on AArch64, PowerPC, s390x and RISC-V, among others.
    pub char_signed: bool,
    /// Width of `short`.
    pub short_bits: u32,
    /// Width of `int`.
    pub int_bits: u32,
    /// Width of `long`.
    pub long_bits: u32,
    /// Width of `long long`.
    pub long_long_bits: u32,
    /// Width of a pointer; also the width `size_t` is derived from.
    pub ptr_bits: u32,
}

impl TargetModel {
    /// The LP64 model used by 64-bit Unix systems: 32-bit `int`, 64-bit `long`
    /// and pointers.
    pub const LP64: Self = Self {
        char_signed: true,
        short_bits: 16,
        int_bits: 32,
        long_bits: 64,
        long_long_bits: 64,
        ptr_bits: 64,
    };

    /// The ILP32 model used by 32-bit systems, and by 64-bit Windows for
    /// everything but pointers.
    pub const ILP32: Self = Self {
        char_signed: true,
        short_bits: 16,
        int_bits: 32,
        long_bits: 32,
        long_long_bits: 64,
        ptr_bits: 32,
    };

    /// The model of the machine this crate was compiled for.
    ///
    /// See the [module documentation](self) for what this means when
    /// cross-compiling.
    pub const fn host() -> Self {
        Self {
            // The platforms whose ABI makes plain `char` unsigned; this is the
            // same list `core::ffi::c_char` is defined by.
            char_signed: !cfg!(any(
                target_arch = "aarch64",
                target_arch = "arm",
                target_arch = "csky",
                target_arch = "hexagon",
                target_arch = "loongarch64",
                target_arch = "msp430",
                target_arch = "powerpc",
                target_arch = "powerpc64",
                target_arch = "riscv32",
                target_arch = "riscv64",
                target_arch = "s390x",
                target_arch = "xtensa",
            )),
            short_bits: 16,
            int_bits: if cfg!(any(target_arch = "avr", target_arch = "msp430")) {
                16
            } else {
                32
            },
            // `long` is 64 bits on 64-bit Unix (LP64) and 32 everywhere else,
            // including 64-bit Windows (LLP64).
            long_bits: if cfg!(target_pointer_width = "64") && !cfg!(windows) {
                64
            } else {
                32
            },
            long_long_bits: 64,
            ptr_bits: if cfg!(target_pointer_width = "64") {
                64
            } else if cfg!(target_pointer_width = "32") {
                32
            } else {
                16
            },
        }
    }
}

impl Default for TargetModel {
    fn default() -> Self {
        Self::host()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_model_is_self_consistent() {
        let t = TargetModel::host();
        assert!(t.short_bits <= t.int_bits);
        assert!(t.int_bits <= t.long_bits);
        assert!(t.long_bits <= t.long_long_bits);
        assert!(t.long_long_bits >= 64);
    }

    #[test]
    fn a_64_bit_unix_host_is_lp64() {
        if cfg!(all(
            target_pointer_width = "64",
            unix,
            target_arch = "x86_64"
        )) {
            assert_eq!(TargetModel::host(), TargetModel::LP64);
        }
    }
}
