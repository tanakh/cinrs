//! The atomic builtins: GCC's `__atomic_*` and `__sync_*`, and Clang's
//! `__c11_atomic_*`.
//!
//! All three families are *overloaded*: the type of the object is read off the
//! pointer the call is given rather than written into the name, which is why
//! they are handled here rather than declared as functions. What each becomes
//! is one [`ir::ExprKind::Atomic`] node holding the operation, the
//! [class](ir::AtomicClass) of Rust atomic it goes through and the memory
//! orders, all of them resolved; see [`crate::codegen`] for the Rust it turns
//! into.
//!
//! # The three families
//!
//! * **`__atomic_*`** (GCC 4.7) takes the memory order as an argument and works
//!   on a pointer to any of the types [`ir::AtomicClass`] covers, `_Atomic` or
//!   not. Its arithmetic on a *pointer* object is in **bytes**: GCC's
//!   `__atomic_fetch_add(&p, 4, o)` moves `p` on by four bytes whatever it
//!   points at.
//! * **`__sync_*`** (GCC 4.1) is the older family. Every one of them is
//!   sequentially consistent, and the trailing arguments GCC allows — a list of
//!   memory locations the barrier covers — are evaluated and ignored.
//! * **`__c11_atomic_*`** is Clang's, and is what the bundled `<stdatomic.h>`
//!   is written in terms of, exactly as Clang's own header is. It requires the
//!   object to be `_Atomic`, and its arithmetic on a pointer object is
//!   **scaled** by the pointee's size, which is what C11 7.17.7.5 requires of
//!   `atomic_fetch_add`. That is the one place the two families disagree, and
//!   the reason both exist here.
//!
//! # What is refused
//!
//! A 16-byte object (`__int128`), because there is no stable `AtomicU128`;
//! arithmetic on a floating object, which GCC also rejects, and arithmetic on
//! a *function* pointer, which C has none of either; an object whose ABI
//! alignment is narrower than its size, which no `from_ptr` may be built on;
//! and a memory order the operation does not allow, which Rust would panic on
//! at run time and which C makes undefined.
//!
//! A function pointer *is* allowed to be loaded, stored, exchanged and
//! compare-exchanged. Its Rust spelling is an `Option<unsafe extern "C"
//! fn(…)>` rather than a raw pointer, so it goes through the same
//! `AtomicPtr<c_void>` as an object pointer with a `transmute` at each end —
//! sound because an `Option<fn>` is pointer-sized and uses the null pointer as
//! its `None`. `AtomicStore(&sqlite3GlobalConfig.xLog, xLog)` is why.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, AtomicClass, AtomicExpr, AtomicOp, AtomicRmw, BinOp, Expr, ExprKind, MemOrder, PlaceKind,
    Ty,
};

use super::{Sema, place_of};

/// Which family a name belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// `__atomic_*`: the order is an argument, pointer arithmetic is in bytes.
    Atomic,
    /// `__sync_*`: sequentially consistent, trailing arguments ignored.
    Sync,
    /// `__c11_atomic_*`: `_Atomic` operands, pointer arithmetic scaled.
    C11,
}

/// The `__atomic_*` names, without the prefix.
pub const ATOMIC_BUILTINS: &[&str] = &[
    "add_fetch",
    "always_lock_free",
    "and_fetch",
    "clear",
    "compare_exchange",
    "compare_exchange_n",
    "exchange",
    "exchange_n",
    "fetch_add",
    "fetch_and",
    "fetch_nand",
    "fetch_or",
    "fetch_sub",
    "fetch_xor",
    "is_lock_free",
    "load",
    "load_n",
    "nand_fetch",
    "or_fetch",
    "signal_fence",
    "store",
    "store_n",
    "sub_fetch",
    "test_and_set",
    "thread_fence",
    "xor_fetch",
];

/// The `__sync_*` names, without the prefix.
pub const SYNC_BUILTINS: &[&str] = &[
    "add_and_fetch",
    "and_and_fetch",
    "bool_compare_and_swap",
    "fetch_and_add",
    "fetch_and_and",
    "fetch_and_nand",
    "fetch_and_or",
    "fetch_and_sub",
    "fetch_and_xor",
    "lock_release",
    "lock_test_and_set",
    "nand_and_fetch",
    "or_and_fetch",
    "sub_and_fetch",
    "synchronize",
    "val_compare_and_swap",
    "xor_and_fetch",
];

/// The `__c11_atomic_*` names, without the prefix.
pub const C11_ATOMIC_BUILTINS: &[&str] = &[
    "compare_exchange_strong",
    "compare_exchange_weak",
    "exchange",
    "fetch_add",
    "fetch_and",
    "fetch_nand",
    "fetch_or",
    "fetch_sub",
    "fetch_xor",
    "init",
    "is_lock_free",
    "load",
    "signal_fence",
    "store",
    "thread_fence",
];

/// Which family `name` belongs to, and what is left of it after the prefix.
///
/// A name with one of the three prefixes that is *not* in its table is not one
/// of ours at all: `__atomic_thing` is an ordinary undeclared identifier, and
/// saying so is a better diagnostic than inventing a builtin for it.
fn family_of(name: &str) -> Option<(Family, &str)> {
    let (family, rest, table) = if let Some(rest) = name.strip_prefix("__c11_atomic_") {
        (Family::C11, rest, C11_ATOMIC_BUILTINS)
    } else if let Some(rest) = name.strip_prefix("__atomic_") {
        (Family::Atomic, rest, ATOMIC_BUILTINS)
    } else {
        let rest = name.strip_prefix("__sync_")?;
        (Family::Sync, rest, SYNC_BUILTINS)
    };
    table.contains(&rest).then_some((family, rest))
}

/// Whether `name` is one of the three families, which is what
/// `__has_builtin` answers from.
pub fn is_atomic_builtin(name: &str) -> bool {
    family_of(name).is_some()
}

/// The value of the `__ATOMIC_*` macro for each order, which is GCC's own
/// numbering and what the argument is matched against.
fn order_of(value: i128) -> Option<MemOrder> {
    Some(match value {
        0 => MemOrder::Relaxed,
        // `__ATOMIC_CONSUME`. No compiler implements dependency ordering, and
        // Rust has no `Consume`; every one of them strengthens it to acquire.
        1 | 2 => MemOrder::Acquire,
        3 => MemOrder::Release,
        4 => MemOrder::AcqRel,
        5 => MemOrder::SeqCst,
        _ => return None,
    })
}

/// The `AtomicRmw` a name's operation word spells.
fn rmw_of(word: &str) -> Option<AtomicRmw> {
    Some(match word {
        "add" => AtomicRmw::Add,
        "sub" => AtomicRmw::Sub,
        "and" => AtomicRmw::And,
        "or" => AtomicRmw::Or,
        "xor" => AtomicRmw::Xor,
        "nand" => AtomicRmw::Nand,
        _ => return None,
    })
}

/// One resolved call, before the node is built.
struct Call<'a> {
    /// The whole spelling, for a diagnostic.
    name: &'a str,
    family: Family,
    /// Where the call was written.
    range: SourceRange,
}

impl Sema<'_> {
    /// Checks a call to one of the atomic builtins.
    ///
    /// `None` means the name is not one of them; `Some(None)` means it was and
    /// something was wrong with it.
    pub(super) fn atomic_builtin(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Option<Expr>> {
        let (family, rest) = family_of(name)?;
        let call = Call {
            name,
            family,
            range,
        };
        let result = match family {
            Family::Atomic => self.gcc_atomic(&call, rest, args),
            Family::Sync => self.gcc_sync(&call, rest, args),
            Family::C11 => self.c11_atomic(&call, rest, args),
        };
        // Whatever was evaluated only for its side effects — a memory order
        // that was not a constant, a `__sync_*` builtin's trailing "protected
        // variables" — happens first, as the left operand of a comma.
        let pending = std::mem::take(&mut self.pending_discard);
        Some(result.map(|expr| self.with_discarded(expr, pending)))
    }

    /// Puts the evaluated-and-ignored operands in front of the node.
    fn with_discarded(&mut self, expr: Expr, discarded: Vec<Expr>) -> Expr {
        let mut out = expr;
        for value in discarded.into_iter().rev() {
            let (ty, range) = (out.ty, out.range);
            out = Expr::new(
                ExprKind::Comma {
                    lhs: Box::new(value),
                    rhs: Box::new(out),
                },
                ty,
                range,
            );
        }
        out
    }

    /// The `__atomic_*` family.
    fn gcc_atomic(&mut self, call: &Call, rest: &str, args: &[ast::Expr]) -> Option<Expr> {
        match rest {
            "thread_fence" | "signal_fence" => {
                self.arity(call, args, 1)?;
                let order = self.order_arg(call, &args[0], MemOrder::SeqCst);
                self.fence(call, rest == "signal_fence", order)
            }
            "always_lock_free" | "is_lock_free" => self.lock_free(call, args, 2),
            "load_n" | "load" => {
                let generic = rest == "load";
                self.arity(call, args, if generic { 3 } else { 2 })?;
                let object = self.atomic_object(call, &args[0])?;
                let order = self.order_arg(call, &args[args.len() - 1], MemOrder::SeqCst);
                self.check_load_order(call, order, args[args.len() - 1].range)?;
                let load = self.atomic_node(AtomicOp::Load, &object, None, None, order, order);
                if !generic {
                    return Some(load);
                }
                self.store_through(call, &args[1], load)
            }
            "store_n" | "store" => {
                let generic = rest == "store";
                self.arity(call, args, 3)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.value_arg(call, &args[1], &object, generic)?;
                let order = self.order_arg(call, &args[2], MemOrder::SeqCst);
                self.check_store_order(call, order, args[2].range)?;
                Some(self.atomic_node(AtomicOp::Store, &object, Some(value), None, order, order))
            }
            "exchange_n" | "exchange" => {
                let generic = rest == "exchange";
                self.arity(call, args, if generic { 4 } else { 3 })?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.value_arg(call, &args[1], &object, generic)?;
                let order = self.order_arg(call, &args[args.len() - 1], MemOrder::SeqCst);
                let swap =
                    self.atomic_node(AtomicOp::Exchange, &object, Some(value), None, order, order);
                if !generic {
                    return Some(swap);
                }
                self.store_through(call, &args[2], swap)
            }
            "compare_exchange_n" | "compare_exchange" => {
                let generic = rest == "compare_exchange";
                self.arity(call, args, 6)?;
                let object = self.atomic_object(call, &args[0])?;
                let expected = self.expected_arg(call, &args[1], &object)?;
                let desired = self.value_arg(call, &args[2], &object, generic)?;
                let weak = self.flag_arg(&args[3]);
                let (success, failure) = self.cas_orders(call, &args[4], &args[5])?;
                Some(self.atomic_node(
                    AtomicOp::CompareExchange { weak },
                    &object,
                    Some(desired),
                    Some(expected),
                    success,
                    failure,
                ))
            }
            "test_and_set" | "clear" => {
                self.arity(call, args, 2)?;
                let object = self.byte_object(call, &args[0])?;
                let order = self.order_arg(call, &args[1], MemOrder::SeqCst);
                if rest == "clear" {
                    self.check_store_order(call, order, args[1].range)?;
                    return Some(self.atomic_node(
                        AtomicOp::Clear,
                        &object,
                        None,
                        None,
                        order,
                        order,
                    ));
                }
                Some(self.atomic_node(AtomicOp::TestAndSet, &object, None, None, order, order))
            }
            _ => {
                let (word, returns_new) = match rest.strip_prefix("fetch_") {
                    Some(word) => (word, false),
                    None => (rest.strip_suffix("_fetch")?, true),
                };
                let op = rmw_of(word)?;
                self.arity(call, args, 3)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.rmw_operand(call, &args[1], &object, op, false)?;
                let order = self.order_arg(call, &args[2], MemOrder::SeqCst);
                Some(self.atomic_node(
                    AtomicOp::Rmw { op, returns_new },
                    &object,
                    Some(value),
                    None,
                    order,
                    order,
                ))
            }
        }
    }

    /// The `__sync_*` family, every one of them sequentially consistent.
    fn gcc_sync(&mut self, call: &Call, rest: &str, args: &[ast::Expr]) -> Option<Expr> {
        let seq = MemOrder::SeqCst;
        match rest {
            "synchronize" => {
                self.discard_rest(args, 0);
                self.fence(call, false, seq)
            }
            "lock_release" => {
                self.at_least(call, args, 1)?;
                let object = self.atomic_object(call, &args[0])?;
                self.discard_rest(args, 1);
                let value = self.zero_operand(&object, call.range);
                Some(self.atomic_node(
                    AtomicOp::Store,
                    &object,
                    Some(value),
                    None,
                    MemOrder::Release,
                    MemOrder::Release,
                ))
            }
            "lock_test_and_set" => {
                self.at_least(call, args, 2)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.value_arg(call, &args[1], &object, false)?;
                self.discard_rest(args, 2);
                Some(self.atomic_node(
                    AtomicOp::Exchange,
                    &object,
                    Some(value),
                    None,
                    MemOrder::Acquire,
                    MemOrder::Acquire,
                ))
            }
            "bool_compare_and_swap" | "val_compare_and_swap" => {
                self.at_least(call, args, 3)?;
                let object = self.atomic_object(call, &args[0])?;
                let old = self.value_arg(call, &args[1], &object, false)?;
                let new = self.value_arg(call, &args[2], &object, false)?;
                self.discard_rest(args, 3);
                let value_is_old = rest.starts_with("val");
                Some(self.atomic_node(
                    AtomicOp::SyncCompareSwap { value_is_old },
                    &object,
                    Some(new),
                    Some(old),
                    seq,
                    seq,
                ))
            }
            _ => {
                let (word, returns_new) = match rest.strip_prefix("fetch_and_") {
                    Some(word) => (word, false),
                    None => (rest.strip_suffix("_and_fetch")?, true),
                };
                let op = rmw_of(word)?;
                self.at_least(call, args, 2)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.rmw_operand(call, &args[1], &object, op, false)?;
                self.discard_rest(args, 2);
                Some(self.atomic_node(
                    AtomicOp::Rmw { op, returns_new },
                    &object,
                    Some(value),
                    None,
                    seq,
                    seq,
                ))
            }
        }
    }

    /// Clang's `__c11_atomic_*` family, which `<stdatomic.h>` is written in.
    fn c11_atomic(&mut self, call: &Call, rest: &str, args: &[ast::Expr]) -> Option<Expr> {
        match rest {
            "thread_fence" | "signal_fence" => {
                self.arity(call, args, 1)?;
                let order = self.order_arg(call, &args[0], MemOrder::SeqCst);
                self.fence(call, rest == "signal_fence", order)
            }
            "is_lock_free" => self.lock_free(call, args, 1),
            // `atomic_init`, which is a plain store: C11 7.17.2.2 says
            // initialising an atomic object is not itself an atomic operation.
            "init" => {
                self.arity(call, args, 2)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.value_arg(call, &args[1], &object, false)?;
                let place = place_of(
                    PlaceKind::Deref(Box::new(object.ptr.clone())),
                    object.value_ty,
                    false,
                    call.range,
                );
                Some(Expr::new(
                    ExprKind::Assign {
                        place,
                        value: Box::new(value),
                    },
                    object.value_ty,
                    call.range,
                ))
            }
            "load" => {
                self.arity(call, args, 2)?;
                let object = self.atomic_object(call, &args[0])?;
                let order = self.order_arg(call, &args[1], MemOrder::SeqCst);
                self.check_load_order(call, order, args[1].range)?;
                Some(self.atomic_node(AtomicOp::Load, &object, None, None, order, order))
            }
            "store" => {
                self.arity(call, args, 3)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.value_arg(call, &args[1], &object, false)?;
                let order = self.order_arg(call, &args[2], MemOrder::SeqCst);
                self.check_store_order(call, order, args[2].range)?;
                Some(self.atomic_node(AtomicOp::Store, &object, Some(value), None, order, order))
            }
            "exchange" => {
                self.arity(call, args, 3)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.value_arg(call, &args[1], &object, false)?;
                let order = self.order_arg(call, &args[2], MemOrder::SeqCst);
                Some(self.atomic_node(AtomicOp::Exchange, &object, Some(value), None, order, order))
            }
            "compare_exchange_strong" | "compare_exchange_weak" => {
                self.arity(call, args, 5)?;
                let object = self.atomic_object(call, &args[0])?;
                let expected = self.expected_arg(call, &args[1], &object)?;
                let desired = self.value_arg(call, &args[2], &object, false)?;
                let (success, failure) = self.cas_orders(call, &args[3], &args[4])?;
                let weak = rest.ends_with("weak");
                Some(self.atomic_node(
                    AtomicOp::CompareExchange { weak },
                    &object,
                    Some(desired),
                    Some(expected),
                    success,
                    failure,
                ))
            }
            _ => {
                let word = rest.strip_prefix("fetch_")?;
                let op = rmw_of(word)?;
                self.arity(call, args, 3)?;
                let object = self.atomic_object(call, &args[0])?;
                let value = self.rmw_operand(call, &args[1], &object, op, true)?;
                let order = self.order_arg(call, &args[2], MemOrder::SeqCst);
                Some(self.atomic_node(
                    AtomicOp::Rmw {
                        op,
                        returns_new: false,
                    },
                    &object,
                    Some(value),
                    None,
                    order,
                    order,
                ))
            }
        }
    }

    // -- the pieces ---------------------------------------------------------

    /// Builds the node.
    fn atomic_node(
        &mut self,
        op: AtomicOp,
        object: &AtomicObject,
        value: Option<Expr>,
        expected: Option<Expr>,
        success: MemOrder,
        failure: MemOrder,
    ) -> Expr {
        let ty = match op {
            AtomicOp::Load | AtomicOp::Exchange => object.value_ty,
            AtomicOp::Rmw { .. } => object.value_ty,
            AtomicOp::SyncCompareSwap { value_is_old: true } => object.value_ty,
            AtomicOp::SyncCompareSwap { .. } | AtomicOp::CompareExchange { .. } => Ty::Bool,
            AtomicOp::TestAndSet => Ty::Bool,
            AtomicOp::Store | AtomicOp::Clear | AtomicOp::Fence { .. } => Ty::Void,
        };
        let range = object.range;
        Expr::new(
            ExprKind::Atomic(Box::new(AtomicExpr {
                op,
                class: object.class,
                value_ty: object.value_ty,
                ptr: Some(object.ptr.clone()),
                value,
                expected,
                success,
                failure,
            })),
            ty,
            range,
        )
    }

    /// A fence, which has no object at all — and therefore no class either,
    /// which is what the unused `Int` below stands for.
    fn fence(&mut self, call: &Call, signal: bool, order: MemOrder) -> Option<Expr> {
        Some(Expr::new(
            ExprKind::Atomic(Box::new(AtomicExpr {
                op: AtomicOp::Fence { signal },
                class: AtomicClass::Int {
                    bytes: 4,
                    signed: true,
                },
                value_ty: Ty::Void,
                ptr: None,
                value: None,
                expected: None,
                success: order,
                failure: order,
            })),
            Ty::Void,
            call.range,
        ))
    }

    /// `__atomic_always_lock_free(size, ptr)` and its two relatives, which are
    /// constants: every size a Rust atomic exists for is lock free, and no
    /// other size is supported at all.
    fn lock_free(&mut self, call: &Call, args: &[ast::Expr], arity: usize) -> Option<Expr> {
        self.arity(call, args, arity)?;
        let size = self.expr(&args[0])?;
        for arg in &args[1..] {
            self.expr(arg);
        }
        let answer = match self.const_eval(&size) {
            Some(ir::ConstValue::Int(bytes)) => {
                i128::from(matches!(bytes, 1 | 2 | 4 | 8) && bytes <= self.max_atomic_bytes())
            }
            // GCC answers a size it cannot see at compile time with a call to
            // the library; there is none here, and a "no" is the honest
            // answer for a size that might be anything.
            _ => 0,
        };
        Some(Expr::int(answer, Ty::Bool, call.range))
    }

    /// The widest atomic this target can perform, which is what an object's
    /// ABI alignment allows: the i386 System V ABI aligns an eight-byte scalar
    /// to four, and `AtomicU64::from_ptr` may not be handed one of those.
    fn max_atomic_bytes(&self) -> i128 {
        i128::from(self.target.max_scalar_align).min(8)
    }

    /// Checks the pointer argument and works out what kind of atomic the
    /// object is reached through.
    fn atomic_object(&mut self, call: &Call, arg: &ast::Expr) -> Option<AtomicObject> {
        let ptr = self.expr(arg)?;
        let Some(pointee) = self.pointee(ptr.ty) else {
            self.error(
                arg.range,
                format!(
                    "the first argument of '{}' must be a pointer, not '{}'",
                    call.name,
                    self.tyname(ptr.ty)
                ),
            );
            return None;
        };
        if call.family == Family::C11 && !pointee.is_atomic() {
            self.error(
                arg.range,
                format!(
                    "the first argument of '{}' must be a pointer to an '_Atomic' type, \
                     and '{}' is not",
                    call.name,
                    self.tyname(ptr.ty)
                ),
            );
            return None;
        }
        let value_ty = self.types().unatomic(pointee);
        let class = self.atomic_class_of(call, value_ty, arg.range)?;
        Some(AtomicObject {
            ptr,
            value_ty,
            class,
            range: call.range,
        })
    }

    /// The same, for `__atomic_test_and_set` and `__atomic_clear`, which work
    /// on one byte of anything at all.
    fn byte_object(&mut self, call: &Call, arg: &ast::Expr) -> Option<AtomicObject> {
        let ptr = self.expr(arg)?;
        let Some(pointee) = self.pointee(ptr.ty) else {
            self.error(
                arg.range,
                format!(
                    "the first argument of '{}' must be a pointer, not '{}'",
                    call.name,
                    self.tyname(ptr.ty)
                ),
            );
            return None;
        };
        let value_ty = self.types().unatomic(pointee);
        // GCC takes a pointer to anything and touches its first byte; a type
        // whose first byte is not a whole object of its own would make the
        // generated `AtomicU8` a lie about what is being written, so this is
        // narrowed to the byte-sized scalars — which is every `atomic_flag`
        // and every `char` lock a program really writes.
        if self.size_of(value_ty) != Some(1) || !value_ty.is_scalar() {
            self.error(
                arg.range,
                format!(
                    "'{}' needs a pointer to a one-byte object, and '{}' points at '{}'",
                    call.name,
                    self.tyname(ptr.ty),
                    self.tyname(value_ty)
                ),
            );
            return None;
        }
        Some(AtomicObject {
            ptr,
            value_ty,
            class: AtomicClass::Int {
                bytes: 1,
                signed: false,
            },
            range: call.range,
        })
    }

    /// Which Rust atomic a C type is reached through, with the reason it is
    /// not one of them where it is not.
    fn atomic_class_of(&mut self, call: &Call, ty: Ty, range: SourceRange) -> Option<AtomicClass> {
        if let Some(class) = ir::atomic_class(self.types(), ty, &self.target) {
            if let Some(bytes) = self.size_of(ty)
                && i128::from(bytes) > self.max_atomic_bytes()
            {
                self.error(
                    range,
                    format!(
                        "'{}' on '{}' is not supported on this target: the object is {bytes} \
                         bytes and this ABI aligns it to {}, which a lock-free atomic of that \
                         width cannot be built on",
                        call.name,
                        self.tyname(ty),
                        self.target.max_scalar_align
                    ),
                );
                return None;
            }
            return Some(class);
        }
        let reason = if ty.is_int128() {
            "there is no stable 128-bit atomic in `core::sync::atomic`"
        } else if ty.is_record() || ty.is_array() {
            "only the scalar types have a lock-free atomic in `core::sync::atomic`"
        } else {
            "there is no atomic of that type in `core::sync::atomic`"
        };
        self.error(
            range,
            format!(
                "'{}' cannot operate on '{}': {reason}",
                call.name,
                self.tyname(ty)
            ),
        );
        None
    }

    /// The value operand of a store, an exchange or a compare-and-exchange.
    ///
    /// `indirect` is the `_n`-less form's, where the operand is a *pointer* to
    /// the value and the value is read out of it.
    fn value_arg(
        &mut self,
        call: &Call,
        arg: &ast::Expr,
        object: &AtomicObject,
        indirect: bool,
    ) -> Option<Expr> {
        let value = self.expr(arg)?;
        if !indirect {
            return Some(self.convert(value, object.value_ty));
        }
        let Some(pointee) = self.pointee(value.ty) else {
            self.error(
                arg.range,
                format!(
                    "'{}' takes a pointer to the value, and '{}' is not one",
                    call.name,
                    self.tyname(value.ty)
                ),
            );
            return None;
        };
        let pointee = self.types().unatomic(pointee);
        let place = place_of(PlaceKind::Deref(Box::new(value)), pointee, false, arg.range);
        let loaded = Expr::new(ExprKind::Load(place), pointee, arg.range);
        Some(self.convert(loaded, object.value_ty))
    }

    /// The `expected` operand of `__atomic_compare_exchange`, which is a
    /// pointer the observed value is written back through.
    fn expected_arg(
        &mut self,
        call: &Call,
        arg: &ast::Expr,
        object: &AtomicObject,
    ) -> Option<Expr> {
        let ptr = self.expr(arg)?;
        let Some(pointee) = self.pointee(ptr.ty) else {
            self.error(
                arg.range,
                format!(
                    "the 'expected' argument of '{}' must be a pointer, not '{}'",
                    call.name,
                    self.tyname(ptr.ty)
                ),
            );
            return None;
        };
        if self.types().unatomic(pointee) != object.value_ty {
            self.error(
                arg.range,
                format!(
                    "the 'expected' argument of '{}' must point at '{}', not at '{}'",
                    call.name,
                    self.tyname(object.value_ty),
                    self.tyname(pointee)
                ),
            );
            return None;
        }
        Some(ptr)
    }

    /// The operand of a read-modify-write, with the checks that depend on what
    /// the object is.
    fn rmw_operand(
        &mut self,
        call: &Call,
        arg: &ast::Expr,
        object: &AtomicObject,
        op: AtomicRmw,
        scaled: bool,
    ) -> Option<Expr> {
        let value = self.expr(arg)?;
        match object.class {
            // GCC refuses arithmetic on a floating object too: there is no
            // `lock xadd` for one, and a compare-exchange loop is not what the
            // builtin promises.
            AtomicClass::Float { .. } => {
                self.error(
                    call.range,
                    format!(
                        "'{}' does not work on '{}': the atomic arithmetic builtins take an \
                         integer or a pointer object",
                        call.name,
                        self.tyname(object.value_ty)
                    ),
                );
                None
            }
            // A function pointer may be loaded, stored and exchanged, but not
            // added to: C has no arithmetic on one at all, the pointee has no
            // size, and Rust has no `wrapping_byte_offset` for an `fn`.
            AtomicClass::FnPtr => {
                self.error(
                    call.range,
                    format!(
                        "'{}' does not work on '{}': there is no arithmetic on a function \
                         pointer",
                        call.name,
                        self.tyname(object.value_ty)
                    ),
                );
                None
            }
            AtomicClass::Ptr => {
                if !matches!(op, AtomicRmw::Add | AtomicRmw::Sub) {
                    self.error(
                        call.range,
                        format!(
                            "'{}' does not work on a pointer object: only '+' and '-' do",
                            call.name
                        ),
                    );
                    return None;
                }
                if !value.ty.is_integer() {
                    self.error(
                        arg.range,
                        format!(
                            "'{}' on a pointer object takes an integer operand, not '{}'",
                            call.name,
                            self.tyname(value.ty)
                        ),
                    );
                    return None;
                }
                let diff = Ty::ptrdiff_ty(&self.target);
                let value = self.convert(value, diff);
                if !scaled {
                    return Some(value);
                }
                // C11 7.17.7.5 counts in *elements*, so the byte count the
                // node carries is the operand times the pointee's size. The
                // `__atomic_*` family does not scale, which is the documented
                // difference between the two.
                let pointee = self.pointee(object.value_ty)?;
                let size = self.size_of(pointee).unwrap_or(1).max(1);
                if size == 1 {
                    return Some(value);
                }
                let scale = Expr::int(i128::from(size), diff, arg.range);
                Some(Expr::new(
                    ExprKind::Binary {
                        op: BinOp::Mul,
                        lhs: Box::new(value),
                        rhs: Box::new(scale),
                    },
                    diff,
                    arg.range,
                ))
            }
            AtomicClass::Bool => {
                if matches!(op, AtomicRmw::Add | AtomicRmw::Sub) {
                    self.error(
                        call.range,
                        format!(
                            "'{}' does not work on a '_Bool' object: adding to one has no \
                             meaning",
                            call.name
                        ),
                    );
                    return None;
                }
                Some(self.convert(value, Ty::Bool))
            }
            AtomicClass::Int { .. } => {
                if !value.ty.is_integer() {
                    self.error(
                        arg.range,
                        format!(
                            "'{}' takes an integer operand, not '{}'",
                            call.name,
                            self.tyname(value.ty)
                        ),
                    );
                    return None;
                }
                Some(self.convert(value, object.value_ty))
            }
        }
    }

    /// The all-bits-zero value of an object's type, which is what
    /// `__sync_lock_release` stores.
    fn zero_operand(&mut self, object: &AtomicObject, range: SourceRange) -> Expr {
        Expr::new(ExprKind::Zeroed, object.value_ty, range)
    }

    /// Resolves a memory-order argument.
    ///
    /// C requires an integer constant expression here and GCC does not: it
    /// takes a run-time value and falls back to `__ATOMIC_SEQ_CST`, and so
    /// does this — the expression is still evaluated, since C says it is.
    fn order_arg(&mut self, call: &Call, arg: &ast::Expr, fallback: MemOrder) -> MemOrder {
        let Some(value) = self.expr(arg) else {
            return fallback;
        };
        let Some(ir::ConstValue::Int(order)) = self.const_eval(&value) else {
            self.pending_discard.push(value);
            return fallback;
        };
        match order_of(order) {
            Some(order) => order,
            None => {
                self.error(
                    arg.range,
                    format!(
                        "'{}' has no memory order {order}; write one of the '__ATOMIC_…' \
                         macros or a 'memory_order_…' constant",
                        call.name
                    ),
                );
                fallback
            }
        }
    }

    /// The success and failure orders of a compare-and-exchange, with C's two
    /// rules about the second one (C11 7.17.7.4p2).
    fn cas_orders(
        &mut self,
        call: &Call,
        success: &ast::Expr,
        failure: &ast::Expr,
    ) -> Option<(MemOrder, MemOrder)> {
        let ok = self.order_arg(call, success, MemOrder::SeqCst);
        let bad = self.order_arg(call, failure, MemOrder::SeqCst);
        if !bad.valid_for_load() {
            self.error(
                failure.range,
                format!(
                    "the failure memory order of '{}' may not be '{}': it describes a load",
                    call.name,
                    bad.c_name()
                ),
            );
            return None;
        }
        if bad.strength() > ok.strength() {
            self.error(
                failure.range,
                format!(
                    "the failure memory order of '{}' may not be stronger than the success \
                     order ('{}' against '{}')",
                    call.name,
                    bad.c_name(),
                    ok.c_name()
                ),
            );
            return None;
        }
        Some((ok, bad))
    }

    /// C11 7.17.7.2p3: a load may not be a release.
    fn check_load_order(&mut self, call: &Call, order: MemOrder, range: SourceRange) -> Option<()> {
        if order.valid_for_load() {
            return Some(());
        }
        self.error(
            range,
            format!(
                "'{}' may not be performed with '{}': it is a load, and a load has nothing \
                 to release",
                call.name,
                order.c_name()
            ),
        );
        None
    }

    /// C11 7.17.7.1p2: a store may not be an acquire.
    fn check_store_order(
        &mut self,
        call: &Call,
        order: MemOrder,
        range: SourceRange,
    ) -> Option<()> {
        if order.valid_for_store() {
            return Some(());
        }
        self.error(
            range,
            format!(
                "'{}' may not be performed with '{}': it is a store, and a store has nothing \
                 to acquire",
                call.name,
                order.c_name()
            ),
        );
        None
    }

    /// The `weak` flag of a compare-and-exchange, which GCC also takes as a
    /// run-time value and which is a `false` when it is not a constant.
    fn flag_arg(&mut self, arg: &ast::Expr) -> bool {
        let Some(value) = self.expr(arg) else {
            return false;
        };
        match self.const_eval(&value) {
            Some(ir::ConstValue::Int(flag)) => flag != 0,
            _ => {
                self.pending_discard.push(value);
                false
            }
        }
    }

    /// Stores the value an `_n`-less form produces through its result pointer.
    fn store_through(&mut self, call: &Call, arg: &ast::Expr, value: Expr) -> Option<Expr> {
        let ptr = self.expr(arg)?;
        let Some(pointee) = self.pointee(ptr.ty) else {
            self.error(
                arg.range,
                format!(
                    "'{}' writes its result through a pointer, and '{}' is not one",
                    call.name,
                    self.tyname(ptr.ty)
                ),
            );
            return None;
        };
        let pointee = self.types().unatomic(pointee);
        let place = place_of(PlaceKind::Deref(Box::new(ptr)), pointee, false, arg.range);
        let value = self.convert(value, pointee);
        Some(Expr::new(
            ExprKind::Assign {
                place,
                value: Box::new(value),
            },
            pointee,
            call.range,
        ))
    }

    /// Evaluates the arguments a `__sync_*` builtin allows past the ones it
    /// uses — GCC's "list of variables to be protected" — and keeps them so
    /// that their side effects survive.
    fn discard_rest(&mut self, args: &[ast::Expr], from: usize) {
        for arg in args.iter().skip(from) {
            if let Some(value) = self.expr(arg) {
                self.pending_discard.push(value);
            }
        }
    }

    /// The argument count, which is fixed for every form of the two families
    /// that have one.
    fn arity(&mut self, call: &Call, args: &[ast::Expr], wanted: usize) -> Option<()> {
        if args.len() == wanted {
            return Some(());
        }
        self.error(
            call.range,
            format!(
                "'{}' expects {wanted} argument{}, have {}",
                call.name,
                if wanted == 1 { "" } else { "s" },
                args.len()
            ),
        );
        None
    }

    /// The minimum argument count of a `__sync_*` builtin, which takes any
    /// number of trailing ones.
    fn at_least(&mut self, call: &Call, args: &[ast::Expr], wanted: usize) -> Option<()> {
        if args.len() >= wanted {
            return Some(());
        }
        self.error(
            call.range,
            format!(
                "'{}' expects at least {wanted} argument{}, have {}",
                call.name,
                if wanted == 1 { "" } else { "s" },
                args.len()
            ),
        );
        None
    }
}

/// The object an atomic builtin works on.
struct AtomicObject {
    /// Its address.
    ptr: Expr,
    /// Its type, with any `_Atomic` taken off.
    value_ty: Ty,
    /// The Rust atomic it is reached through.
    class: AtomicClass,
    /// Where the call was written.
    range: SourceRange,
}
