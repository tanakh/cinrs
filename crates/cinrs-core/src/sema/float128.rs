//! Types that may be named but never have a value: `_Float128`, and the
//! complex types when the `complex` feature is off.
//!
//! binary128 has no Rust type on a stable compiler (`f128` is unstable), so
//! nothing can compute with one. It still has to be *nameable*, because glibc
//! declares `strtof128`, `sinf128`, `csqrtf128` and a few hundred more with it
//! as soon as the compiler claims GCC 7 and `_GNU_SOURCE` asks for them, and
//! its `<math.h>` lists `_Float128` in the `_Generic` behind `issignaling` —
//! where it has to be a type distinct from `double` so that a `double` does
//! not pick the `f128` function.
//!
//! The complex types are the same case when the `complex` feature is off:
//! there is then no runtime for their values, but glibc's `<complex.h>` — which
//! `<tgmath.h>` includes too — declares forty functions with them, and a
//! program that never touches a complex value must not fail on a platform
//! header.
//!
//! So each such type is a struct of its size and alignment — sixteen and
//! sixteen for `_Float128`, thirty-two for `_Complex _Float128`, eight and four
//! for `float _Complex`, sixteen and eight for `double _Complex` — which is what
//! `sizeof` and `_Alignof` report. A `typedef`, a pointer, a prototype and a
//! `_Generic` association are fine. What would make a value is refused, each
//! with the reason: an object of the type (or an array of it), a cast to it,
//! and a call of a function whose prototype mentions it — the platform passes
//! these in registers, and a value of the stand-in would be passed as a struct,
//! in memory. The same shape as the `long double` boundary of
//! [`super::long_double`], except that there is no sibling to redirect to.

use super::Sema;
use crate::capture::SourceRange;
use crate::ir::{Field, Layout, RecordId, RecordKind, RustField, Signature, Ty};

/// What every `_Float128` refusal ends with.
const REASON: &str = "binary128 has no Rust type to become (Rust's 'f128' is unstable), and \
                      mapping it onto 'double' would compute and pass the wrong values";

/// Which stand-in a record is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StandIn {
    Float128,
    Complex,
}

impl Sema<'_> {
    /// The stand-in type for `_Float128` (or `_Complex _Float128`), created the
    /// first time it is named.
    pub(super) fn float128_ty(&mut self, complex: bool, range: SourceRange) -> Ty {
        let slot = usize::from(complex);
        if let Some(id) = self.float128[slot] {
            return Ty::Record(id);
        }
        let ty = if complex {
            self.stand_in(
                "_Complex _Float128",
                "__cinrs_Complex_Float128",
                32,
                16,
                range,
            )
        } else {
            self.stand_in("_Float128", "__cinrs_Float128", 16, 16, range)
        };
        if let Ty::Record(id) = ty {
            self.float128[slot] = Some(id);
        }
        ty
    }

    /// The stand-in type for `float _Complex` (`single`) or `double _Complex`
    /// when the `complex` feature is off.
    pub(super) fn uncomputable_complex_ty(&mut self, single: bool, range: SourceRange) -> Ty {
        let slot = usize::from(!single);
        if let Some(id) = self.no_complex[slot] {
            return Ty::Record(id);
        }
        let ty = if single {
            self.stand_in("float _Complex", "__cinrs_ComplexFloat", 8, 4, range)
        } else {
            self.stand_in("double _Complex", "__cinrs_ComplexDouble", 16, 8, range)
        };
        if let Ty::Record(id) = ty {
            self.no_complex[slot] = Some(id);
        }
        ty
    }

    /// A struct of `size` bytes aligned to `align`, standing for `c_type`.
    fn stand_in(
        &mut self,
        c_type: &'static str,
        rust_name: &str,
        size: u64,
        align: u64,
        range: SourceRange,
    ) -> Ty {
        let id = self.declare_record(RecordKind::Struct, None, range);
        let bytes = self.program.types.array(Ty::UChar, size, false);
        let rust_name = self.reserve_item_name(rust_name);
        let record = self.program.types.record_mut(id);
        record.rust_name = rust_name;
        record.anonymous = false;
        record.stands_for = Some(c_type);
        record.fields = vec![Field {
            name: "__cinrs_bytes".to_owned(),
            anonymous: false,
            ty: bytes,
            is_const: false,
            offset: 0,
            bits: None,
            flexible: false,
            range,
        }];
        record.rust_fields = vec![RustField::Member(0)];
        record.complete = true;
        record.layout = Some(Layout { size, align });
        record.align = Some(align);
        record.rust_align = align;
        Ty::Record(id)
    }

    /// Whether any stand-in has been named in this unit.
    fn any_stand_in(&self) -> bool {
        self.float128
            .iter()
            .chain(&self.no_complex)
            .any(Option::is_some)
    }

    /// Which stand-in `id` is, if it is one.
    fn stand_in_of(&self, id: RecordId) -> Option<StandIn> {
        if self.float128.contains(&Some(id)) {
            Some(StandIn::Float128)
        } else if self.no_complex.contains(&Some(id)) {
            Some(StandIn::Complex)
        } else {
            None
        }
    }

    /// The stand-in whose values `ty` is, or an array of them holds.
    fn stand_in_value(&self, ty: Ty) -> Option<StandIn> {
        match ty {
            Ty::Record(id) => self.stand_in_of(id),
            Ty::Array(id) => self.stand_in_value(self.types().array_type(id).elem),
            _ => None,
        }
    }

    /// The stand-in `ty` is or reaches through pointers and arrays.
    fn stand_in_mentioned(&self, ty: Ty) -> Option<StandIn> {
        match ty {
            Ty::Record(id) => self.stand_in_of(id),
            Ty::Array(id) => self.stand_in_mentioned(self.types().array_type(id).elem),
            Ty::Pointer(_) => self
                .pointee(ty)
                .and_then(|pointee| self.stand_in_mentioned(pointee)),
            _ => None,
        }
    }

    /// Refuses an object of type `ty` if it holds values of a stand-in.
    pub(super) fn refuse_float128_object(&mut self, name: &str, ty: Ty, range: SourceRange) {
        if !self.any_stand_in() {
            return;
        }
        match self.stand_in_value(ty) {
            Some(StandIn::Float128) => {
                let spelled = self.tyname(ty);
                self.error(
                    range,
                    format!("'{name}' cannot be an object of type '{spelled}': {REASON}"),
                );
            }
            Some(StandIn::Complex) => {
                self.error(range, crate::COMPLEX_UNSUPPORTED.to_owned());
            }
            None => {}
        }
    }

    /// Refuses a cast to `_Float128`, or to a complex type without the
    /// feature.
    pub(super) fn refuse_float128_cast(&mut self, spelled: &str, range: SourceRange) {
        let reason = if spelled.contains("_Float128") {
            REASON
        } else {
            crate::COMPLEX_UNSUPPORTED
        };
        self.error(
            range,
            format!("a cast to '{spelled}' is not supported: {reason}"),
        );
    }

    /// Refuses a call of `name`, whose prototype is `sig`, if the prototype
    /// mentions a stand-in. Returns whether it did.
    pub(super) fn refuse_float128_call(
        &mut self,
        name: &str,
        sig: &Signature,
        range: SourceRange,
    ) -> bool {
        if self.dead_code > 0 || !self.any_stand_in() {
            return false;
        }
        let (offence, which) = if let Some(which) = self.stand_in_mentioned(sig.ret) {
            (format!("returns '{}'", self.tyname(sig.ret)), which)
        } else if let Some((param, which)) = sig
            .params
            .iter()
            .find_map(|p| Some((*p, self.stand_in_mentioned(*p)?)))
        {
            (format!("takes '{}'", self.tyname(param)), which)
        } else {
            return false;
        };
        let reason = match which {
            StandIn::Float128 => REASON,
            StandIn::Complex => crate::COMPLEX_UNSUPPORTED,
        };
        self.error(
            range,
            format!("'{name}' {offence}, which cannot be called: {reason}"),
        );
        true
    }
}
