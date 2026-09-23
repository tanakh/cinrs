//! `_Float128`: a type that may be named but never have a value.
//!
//! binary128 has no Rust type on a stable compiler (`f128` is unstable), so
//! nothing can compute with one. It still has to be *nameable*, because glibc
//! declares `strtof128`, `sinf128`, `csqrtf128` and a few hundred more with it
//! as soon as the compiler claims GCC 7 and `_GNU_SOURCE` asks for them, and
//! its `<math.h>` lists `_Float128` in the `_Generic` behind `issignaling` —
//! where it has to be a type distinct from `double` so that a `double` does
//! not pick the `f128` function.
//!
//! So `_Float128` (and GCC's `__float128`, the same type) is a struct of
//! sixteen bytes aligned to sixteen, standing for binary128 — the size and
//! alignment `sizeof` and `_Alignof` report — and `_Complex _Float128` one of
//! thirty-two. A `typedef`, a pointer, a prototype and a `_Generic`
//! association are fine. What would make a value is refused, each with the
//! reason: an object of the type (or an array of it), a cast to it, and a call
//! of a function whose prototype mentions it — the platform passes a
//! binary128 in an SSE register, and a value of the stand-in would be passed
//! as a struct, in memory. The same shape as the `long double` boundary of
//! [`super::long_double`], except that there is no sibling to redirect to.

use super::Sema;
use crate::capture::SourceRange;
use crate::ir::{Field, Layout, RecordId, RecordKind, RustField, Signature, Ty};

/// What every refusal ends with.
const REASON: &str = "binary128 has no Rust type to become (Rust's 'f128' is unstable), and \
                      mapping it onto 'double' would compute and pass the wrong values";

impl Sema<'_> {
    /// The stand-in type for `_Float128` (or `_Complex _Float128`), created the
    /// first time it is named.
    pub(super) fn float128_ty(&mut self, complex: bool, range: SourceRange) -> Ty {
        let slot = usize::from(complex);
        if let Some(id) = self.float128[slot] {
            return Ty::Record(id);
        }
        let (stands_for, size) = if complex {
            ("_Complex _Float128", 32)
        } else {
            ("_Float128", 16)
        };
        let id = self.declare_record(RecordKind::Struct, None, range);
        let bytes = self.program.types.array(Ty::UChar, size, false);
        let rust_name = self.reserve_item_name(if complex {
            "__cinrs_Complex_Float128"
        } else {
            "__cinrs_Float128"
        });
        let record = self.program.types.record_mut(id);
        record.rust_name = rust_name;
        record.anonymous = false;
        record.stands_for = Some(stands_for);
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
        record.layout = Some(Layout { size, align: 16 });
        record.align = Some(16);
        record.rust_align = 16;
        self.float128[slot] = Some(id);
        Ty::Record(id)
    }

    /// Whether `id` is one of the stand-ins.
    fn is_float128_record(&self, id: RecordId) -> bool {
        self.float128.contains(&Some(id))
    }

    /// Whether `ty` is a `_Float128` value, or an array of them.
    fn float128_value(&self, ty: Ty) -> bool {
        match ty {
            Ty::Record(id) => self.is_float128_record(id),
            Ty::Array(id) => self.float128_value(self.types().array_type(id).elem),
            _ => false,
        }
    }

    /// Whether `ty` is `_Float128`, or reaches one through pointers and
    /// arrays.
    fn mentions_float128(&self, ty: Ty) -> bool {
        match ty {
            Ty::Record(id) => self.is_float128_record(id),
            Ty::Array(id) => self.mentions_float128(self.types().array_type(id).elem),
            Ty::Pointer(_) => self
                .pointee(ty)
                .is_some_and(|pointee| self.mentions_float128(pointee)),
            _ => false,
        }
    }

    /// Refuses an object of type `ty` if it holds `_Float128` values.
    pub(super) fn refuse_float128_object(&mut self, name: &str, ty: Ty, range: SourceRange) {
        if self.float128[0].is_none() && self.float128[1].is_none() {
            return;
        }
        if self.float128_value(ty) {
            let spelled = self.tyname(ty);
            self.error(
                range,
                format!("'{name}' cannot be an object of type '{spelled}': {REASON}"),
            );
        }
    }

    /// Refuses a cast to `_Float128`.
    pub(super) fn refuse_float128_cast(&mut self, complex: bool, range: SourceRange) {
        let spelled = if complex {
            "_Complex _Float128"
        } else {
            "_Float128"
        };
        self.error(
            range,
            format!("a cast to '{spelled}' is not supported: {REASON}"),
        );
    }

    /// Refuses a call of `name`, whose prototype is `sig`, if the prototype
    /// mentions `_Float128`. Returns whether it did.
    pub(super) fn refuse_float128_call(
        &mut self,
        name: &str,
        sig: &Signature,
        range: SourceRange,
    ) -> bool {
        if self.dead_code > 0 || (self.float128[0].is_none() && self.float128[1].is_none()) {
            return false;
        }
        let offence = if self.mentions_float128(sig.ret) {
            format!("returns '{}'", self.tyname(sig.ret))
        } else if let Some(param) = sig.params.iter().find(|p| self.mentions_float128(**p)) {
            format!("takes '{}'", self.tyname(*param))
        } else {
            return false;
        };
        self.error(
            range,
            format!("'{name}' {offence}, which cannot be called: {REASON}"),
        );
        true
    }
}
