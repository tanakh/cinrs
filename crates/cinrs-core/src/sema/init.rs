//! Initialisers, from `= 0` to `{ .p = { 1, 2 }, [3] = 'x' }`.
//!
//! C's initialiser syntax is a little language of its own: braces may be
//! omitted for a nested aggregate (`int a[2][2] = {1, 2, 3, 4}`), designators
//! may jump around (`{ .y = 2, .x = 1 }`), and whatever is left over is zero.
//! The rules are expressed here as a *cursor* over the elements of one braced
//! list: filling an aggregate consumes as many elements as it needs, which is
//! exactly what makes the elided-brace form fall out of the same code that
//! handles the explicit one.
//!
//! # Known limitation
//!
//! A designator naming a sub-object several levels down (`.a.b = 1`) is
//! reported rather than accepted; write nested braces (`.a = { .b = 1 }`)
//! instead.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    ConstValue, Expr, ExprKind, Place, PlaceKind, RecordId, RecordKind, StaticVar, Storage, Ty,
};
use crate::lex::{StrKind, StrLit};

use super::{ConvContext, Sema, place_of};

/// The most elements an aggregate initialiser will lay out one by one.
///
/// An array whose initialiser is entirely zeros becomes `[0; N]` however large
/// it is; this bound is only about the pathological case of a huge array with a
/// handful of non-zero elements, which would otherwise turn into a token stream
/// nothing could compile.
const MAX_INIT_ELEMENTS: u64 = 1 << 20;

/// A position in one braced initialiser list.
struct Cursor<'a> {
    items: &'a [ast::InitItem],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(items: &'a [ast::InitItem]) -> Self {
        Self { items, pos: 0 }
    }

    fn peek(&self) -> Option<&'a ast::InitItem> {
        self.items.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }
}

impl Sema<'_> {
    /// Checks an initialiser against the type of what it initialises.
    pub(super) fn initializer(
        &mut self,
        init: &ast::Initializer,
        ty: Ty,
        name: &str,
    ) -> Option<Expr> {
        match &init.kind {
            ast::InitializerKind::Expr(expr) => {
                if ty.is_array() {
                    if let ast::ExprKind::Str(lit) = &expr.kind {
                        return self.string_initializer(lit, ty, expr.range);
                    }
                    self.error(
                        init.range,
                        format!(
                            "array '{name}' must be initialized with a brace-enclosed list \
                             or a string literal"
                        ),
                    );
                    return None;
                }
                let value = self.expr(expr)?;
                Some(self.convert_for(value, ty, ConvContext::Init(name.to_owned())))
            }
            ast::InitializerKind::List(items) => {
                let mut cursor = Cursor::new(items);
                let value = self.fill(ty, &mut cursor, name, init.range)?;
                if let Some(extra) = cursor.peek() {
                    self.error(extra.range, "excess elements in initializer");
                }
                Some(value)
            }
        }
    }

    /// Checks `(T){ … }` — a C99 compound literal (6.5.2.5).
    ///
    /// A compound literal denotes an *object*, not a value: it may be assigned
    /// to, subscripted, and have its address taken, and the initialiser rules
    /// are the ones a declaration uses, designators and elided braces and all.
    /// Where the object lives is what the translation turns on.
    ///
    /// * **At block scope** it has automatic storage duration and the lifetime
    ///   of the enclosing block, so `&(struct S){1, 2}` is still valid at the
    ///   end of that block. The object is a hidden local
    ///   [`Sema::block_items`] defines at the head of the block, and the place
    ///   carries the initialiser so that it is evaluated *where the literal
    ///   was written*: C's evaluation order survives, and a literal inside a
    ///   loop is rebuilt on every iteration, exactly as C says.
    /// * **At file scope** it has static storage duration, so it becomes a
    ///   `static mut` item of its own and its initialiser has to be a constant
    ///   expression, as it does for every other object with that duration.
    pub(super) fn compound_literal(
        &mut self,
        type_name: &ast::TypeName,
        items: &[ast::InitItem],
        range: SourceRange,
    ) -> Option<Place> {
        let is_const = type_name.ty.qualifiers.is_const;
        let init = ast::Initializer {
            kind: ast::InitializerKind::List(items.to_vec()),
            range,
        };
        // `(T[]){ … }` takes its length from the initialiser, exactly as
        // `T x[] = { … }` does.
        let inferred = matches!(
            type_name.ty.kind,
            ast::TypeKind::Array {
                size: ast::ArraySize::Unspecified,
                ..
            }
        );
        let (ty, value) = if inferred {
            let ast::TypeKind::Array { elem, .. } = &type_name.ty.kind else {
                unreachable!("just matched");
            };
            let element = self.ty_of(elem)?;
            if self.types().is_vla(element) {
                self.error(type_name.range, super::VM_UNSUPPORTED);
                return None;
            }
            let elem_const = elem.qualifiers.is_const;
            self.init_array_inferred(&init, element, elem_const, "compound literal")?
        } else {
            let ty = self.ty_of(&type_name.ty)?;
            // C99 6.5.2.5p1: the type name may not be a variable length array,
            // and an initialiser could not say how many elements it has.
            if self.types().is_vla(ty) {
                self.error(
                    type_name.range,
                    "a compound literal cannot have a variable length array type",
                );
                return None;
            }
            if ty.is_void() || ty.is_func() || !self.types().is_complete(ty) {
                self.error(
                    type_name.range,
                    format!(
                        "a compound literal needs a complete object type, and '{}' is not one",
                        self.tyname(ty)
                    ),
                );
                return None;
            }
            if self.reject_va_list(ty, type_name.range) {
                return None;
            }
            let value = self.initializer(&init, ty, "compound literal")?;
            (ty, value)
        };

        if self.at_file_scope() {
            let value = self.static_init(value, "the initializer of a compound literal")?;
            let item_name = self.anonymous_name("literal");
            let storage = Storage::Static {
                item_name: item_name.clone(),
                exported: false,
            };
            let id = self.new_object(&item_name, ty, storage, is_const, range);
            self.static_literals.insert(id, self.program.statics.len());
            self.program.statics.push(StaticVar {
                object: id,
                init: value,
            });
            return Some(place_of(PlaceKind::Object(id), ty, is_const, range));
        }

        let name = self.anonymous_name("literal");
        let id = self.new_object(&name, ty, Storage::Automatic, is_const, range);
        self.compound_literals.push(id);
        Some(place_of(
            PlaceKind::CompoundLiteral {
                object: id,
                init: Box::new(value),
            },
            ty,
            is_const,
            range,
        ))
    }

    /// Builds the type and the value of `T x[] = …`, whose length comes from
    /// the initialiser.
    pub(super) fn init_array_inferred(
        &mut self,
        init: &ast::Initializer,
        elem: Ty,
        elem_const: bool,
        name: &str,
    ) -> Option<(Ty, Expr)> {
        if elem.is_func() || !self.types().is_complete(elem) {
            self.error(
                init.range,
                format!("array has incomplete element type '{}'", self.tyname(elem)),
            );
            return None;
        }
        let (values, len) = match &init.kind {
            ast::InitializerKind::Expr(ast::Expr {
                kind: ast::ExprKind::Str(lit),
                ..
            }) => {
                let len = lit.values.len() as u64 + 1;
                let ty = self.program.types.array(elem, len, elem_const);
                let value = self.string_initializer(lit, ty, init.range)?;
                return Some((ty, value));
            }
            ast::InitializerKind::List(items) => {
                let mut cursor = Cursor::new(items);
                // `char s[] = { "hi" }`: C99 6.7.8p14 lets the string literal
                // that initialises a character array be wrapped in braces.
                if let Some(lit) = braced_string(&cursor).filter(|lit| fills_array(elem, lit)) {
                    let len = lit.values.len() as u64 + 1;
                    let ty = self.program.types.array(elem, len, elem_const);
                    let value = self.string_initializer(lit, ty, init.range)?;
                    cursor.advance();
                    if let Some(extra) = cursor.peek() {
                        self.error(extra.range, "excess elements in initializer");
                    }
                    return Some((ty, value));
                }
                let filled = self.fill_array(elem, None, &mut cursor, name, init.range)?;
                if let Some(extra) = cursor.peek() {
                    self.error(extra.range, "excess elements in initializer");
                }
                filled
            }
            ast::InitializerKind::Expr(expr) => {
                self.error(
                    expr.range,
                    format!(
                        "array '{name}' must be initialized with a brace-enclosed list \
                         or a string literal"
                    ),
                );
                return None;
            }
        };
        let ty = self.program.types.array(elem, len, elem_const);
        Some((ty, self.assemble_array(values, elem, len, init.range)))
    }

    /// Fills an object of type `ty` from `cursor`, consuming as many elements
    /// as it needs.
    fn fill(
        &mut self,
        ty: Ty,
        cursor: &mut Cursor,
        name: &str,
        range: SourceRange,
    ) -> Option<Expr> {
        match ty {
            Ty::Array(id) => {
                let array = self.types().array_type(id);
                // `char s[4] = { "abc" }` — the braces around the string are
                // C's, not a one-element list of characters.
                if let Some(lit) = braced_string(cursor).filter(|lit| fills_array(array.elem, lit))
                {
                    let value = self.string_initializer(lit, ty, range);
                    cursor.advance();
                    return value;
                }
                let (values, len) =
                    self.fill_array(array.elem, Some(array.len), cursor, name, range)?;
                Some(self.assemble_array(values, array.elem, len, range))
            }
            Ty::Record(id) => self.fill_record(id, cursor, name, range),
            _ => {
                let Some(item) = cursor.peek() else {
                    return Some(self.zero(ty, range));
                };
                if !item.designators.is_empty() {
                    self.error(
                        item.range,
                        "a designator can only appear in an aggregate initializer",
                    );
                    cursor.advance();
                    return None;
                }
                let value = self.initializer(&item.init, ty, name);
                cursor.advance();
                value
            }
        }
    }

    /// Fills one element of an aggregate, following C's rule that the braces
    /// around a nested aggregate may be left out.
    fn fill_element(
        &mut self,
        ty: Ty,
        cursor: &mut Cursor,
        name: &str,
        range: SourceRange,
    ) -> Option<Expr> {
        let Some(item) = cursor.peek() else {
            return Some(self.zero(ty, range));
        };
        let braced = matches!(item.init.kind, ast::InitializerKind::List(_));
        let string = ty.is_array()
            && matches!(
                &item.init.kind,
                ast::InitializerKind::Expr(ast::Expr {
                    kind: ast::ExprKind::Str(_),
                    ..
                })
            );
        // A compound literal initialises the member *whole* rather than being
        // the first of the values its members' braces were left out of —
        // `struct S s = { (inner_t){}, 1 }` gives the first member the
        // literal. c-testsuite `00216` writes exactly that for a member of an
        // empty struct type, which has no members to fill from anything.
        let whole = ty.is_record()
            && matches!(
                &item.init.kind,
                ast::InitializerKind::Expr(ast::Expr {
                    kind: ast::ExprKind::CompoundLiteral { .. },
                    ..
                })
            );
        if braced || string || whole || !(ty.is_array() || ty.is_record()) {
            let value = self.initializer(&item.init, ty, name);
            cursor.advance();
            return value;
        }
        self.fill(ty, cursor, name, range)
    }

    /// Lays out the elements of an array.
    ///
    /// `len` is `None` for `T x[] = { … }`, whose length is whatever the
    /// initialiser reaches.
    fn fill_array(
        &mut self,
        elem: Ty,
        len: Option<u64>,
        cursor: &mut Cursor,
        name: &str,
        range: SourceRange,
    ) -> Option<(Vec<Option<Expr>>, u64)> {
        if len.is_some_and(|len| len > MAX_INIT_ELEMENTS) {
            self.error(
                range,
                format!(
                    "an array of more than {MAX_INIT_ELEMENTS} elements cannot be \
                     initialized element by element"
                ),
            );
            return None;
        }
        let mut slots: Vec<Option<Expr>> = match len {
            Some(len) => (0..len).map(|_| None).collect(),
            None => Vec::new(),
        };
        let mut index: u64 = 0;
        while let Some(item) = cursor.peek() {
            // Running out of room ends the run — but only for an element that
            // takes the next position; a designator may point back into the
            // array from anywhere.
            if item.designators.is_empty() && len.is_some_and(|len| index >= len) {
                break;
            }
            let mut designated = false;
            // The last element GNU's `[low ... high] = v` fills; a plain
            // designator fills only the one it names.
            let mut upto = None;
            if let Some(first) = item.designators.first() {
                match first {
                    ast::Designator::Index(expr) => {
                        let Some(value) = self.designator_index(expr) else {
                            cursor.advance();
                            continue;
                        };
                        index = value;
                        designated = true;
                    }
                    // GNU's range designator, `[1 ... 5] = 0`, which fills
                    // every element of the range with the same value.
                    ast::Designator::Range(low, high) => {
                        let (Some(low), Some(high)) =
                            (self.designator_index(low), self.designator_index(high))
                        else {
                            cursor.advance();
                            continue;
                        };
                        if high < low {
                            self.error(
                                item.range,
                                "empty range designator: the last index is below the first",
                            );
                            cursor.advance();
                            continue;
                        }
                        index = low;
                        upto = Some(high);
                        designated = true;
                    }
                    ast::Designator::Field(field) => {
                        self.error(
                            field.range,
                            "a field designator cannot initialize an array element",
                        );
                        cursor.advance();
                        continue;
                    }
                }
                if item.designators.len() > 1 {
                    self.error(
                        item.range,
                        "a designator naming a nested member is not supported yet; \
                         write nested braces instead",
                    );
                    cursor.advance();
                    continue;
                }
            }
            let last = upto.unwrap_or(index);
            if len.is_some_and(|len| last >= len) || last >= MAX_INIT_ELEMENTS {
                let item_range = item.range;
                self.error(item_range, "array designator index is out of bounds");
                cursor.advance();
                continue;
            }
            let item_range = item.range;
            let value = if designated {
                let value = self.initializer(&item.init, elem, name);
                cursor.advance();
                value?
            } else {
                self.fill_element(elem, cursor, name, item_range)?
            };
            let slot = last as usize;
            if slot >= slots.len() {
                slots.resize_with(slot + 1, || None);
            }
            // A range designator writes one checked value into every element
            // it covers; the value is a constant in every use that matters,
            // and C leaves the number of evaluations unspecified.
            for slot in index..=last {
                slots[slot as usize] = Some(value.clone());
            }
            index = last + 1;
        }
        let length = len.unwrap_or(slots.len() as u64);
        Some((slots, length))
    }

    /// Turns the filled slots into an array value.
    fn assemble_array(
        &mut self,
        slots: Vec<Option<Expr>>,
        elem: Ty,
        len: u64,
        range: SourceRange,
    ) -> Expr {
        let ty = self.program.types.array(elem, len, false);
        // `= {0}` on a large array is how C spells "all zeros", and `[0; N]` is
        // how Rust does; recognising it keeps the expansion readable.
        if slots
            .iter()
            .all(|slot| slot.as_ref().is_none_or(is_zero_value))
        {
            let zero = self.zero(elem, range);
            return Expr::new(
                ExprKind::ArrayRepeat {
                    value: Box::new(zero),
                    len,
                },
                ty,
                range,
            );
        }
        let mut items = Vec::with_capacity(len as usize);
        for slot in slots {
            match slot {
                Some(value) => items.push(value),
                None => {
                    let zero = self.zero(elem, range);
                    items.push(zero);
                }
            }
        }
        while (items.len() as u64) < len {
            let zero = self.zero(elem, range);
            items.push(zero);
        }
        Expr::new(ExprKind::ArrayLit(items), ty, range)
    }

    /// Lays out the members of a `struct` or `union`.
    fn fill_record(
        &mut self,
        record: RecordId,
        cursor: &mut Cursor,
        name: &str,
        range: SourceRange,
    ) -> Option<Expr> {
        let ty = Ty::Record(record);
        let def = self.types().record(record);
        let kind = def.kind;
        let fields: Vec<(Ty, String)> = def.fields.iter().map(|f| (f.ty, f.name.clone())).collect();
        if fields.is_empty() {
            return Some(Expr::new(ExprKind::Zeroed, ty, range));
        }

        if kind == RecordKind::Union {
            // A union initialiser gives exactly one member a value: the first,
            // or whichever one a designator names.
            let Some(item) = cursor.peek() else {
                return Some(Expr::new(ExprKind::Zeroed, ty, range));
            };
            let mut index = 0usize;
            let mut designated = false;
            let mut path: Vec<usize> = Vec::new();
            if let Some(first) = item.designators.first() {
                match first {
                    ast::Designator::Field(field) => {
                        let Some(found) = self.member_path(record, &field.name) else {
                            self.error(
                                field.range,
                                format!(
                                    "no member named '{}' in '{}'",
                                    field.name,
                                    self.tyname(ty)
                                ),
                            );
                            cursor.advance();
                            return None;
                        };
                        index = found[0];
                        path = found;
                        designated = true;
                    }
                    ast::Designator::Index(expr) => {
                        self.error(
                            expr.range,
                            "an array designator cannot initialize a union member",
                        );
                        cursor.advance();
                        return None;
                    }
                    ast::Designator::Range(low, _) => {
                        self.error(
                            low.range,
                            "a range designator cannot initialize a union member",
                        );
                        cursor.advance();
                        return None;
                    }
                }
                if item.designators.len() > 1 {
                    self.error(
                        item.range,
                        "a designator naming a nested member is not supported yet; \
                         write nested braces instead",
                    );
                    cursor.advance();
                    return None;
                }
            }
            let field_ty = if designated {
                self.member_type(record, &path)
            } else {
                fields[index].0
            };
            let item_range = item.range;
            let value = if designated {
                let value = self.initializer(&item.init, field_ty, name);
                cursor.advance();
                value?
            } else {
                self.fill_element(field_ty, cursor, name, item_range)?
            };
            // A designator may name a member of an anonymous member, which is
            // reached through it: `.x = 1` becomes `{ __cinrs_anon0: { x: 1 } }`.
            let value = if path.len() > 1 {
                let Ty::Record(inner) = fields[index].0 else {
                    return None;
                };
                self.place_member(None, inner, &path[1..], value)
            } else {
                value
            };
            return Some(Expr::new(
                ExprKind::UnionLit {
                    record,
                    index,
                    value: Box::new(value),
                },
                ty,
                range,
            ));
        }

        let flexible: Vec<bool> = self
            .types()
            .record(record)
            .fields
            .iter()
            .map(|f| f.flexible)
            .collect();
        let mut slots: Vec<Option<Expr>> = (0..fields.len()).map(|_| None).collect();
        let mut index = 0usize;
        while let Some(item) = cursor.peek() {
            // As for an array: only an element taking the next position runs
            // out of members, since a designator may name any of them.
            if item.designators.is_empty() && index >= fields.len() {
                break;
            }
            let mut designated = false;
            let mut path: Vec<usize> = Vec::new();
            if let Some(first) = item.designators.first() {
                match first {
                    ast::Designator::Field(field) => {
                        let Some(found) = self.member_path(record, &field.name) else {
                            self.error(
                                field.range,
                                format!(
                                    "no member named '{}' in '{}'",
                                    field.name,
                                    self.tyname(ty)
                                ),
                            );
                            cursor.advance();
                            continue;
                        };
                        index = found[0];
                        path = found;
                        designated = true;
                    }
                    ast::Designator::Index(expr) => {
                        self.error(
                            expr.range,
                            "an array designator cannot initialize a struct member",
                        );
                        cursor.advance();
                        continue;
                    }
                    ast::Designator::Range(low, _) => {
                        self.error(
                            low.range,
                            "a range designator cannot initialize a struct member",
                        );
                        cursor.advance();
                        continue;
                    }
                }
                if item.designators.len() > 1 {
                    self.error(
                        item.range,
                        "a designator naming a nested member is not supported yet; \
                         write nested braces instead",
                    );
                    cursor.advance();
                    continue;
                }
            }
            // A flexible array member has no elements, so there is nothing an
            // initialiser could put in it — C99 6.7.2.1p18 says so, and GCC
            // agrees.
            if flexible.get(index).copied().unwrap_or(false) {
                let item_range = item.range;
                self.error(
                    item_range,
                    "a flexible array member cannot be initialized; allocate the object with \
                     room for the elements and fill them in",
                );
                cursor.advance();
                index += 1;
                continue;
            }
            let field_ty = if designated {
                self.member_type(record, &path)
            } else {
                fields[index].0
            };
            let item_range = item.range;
            let value = if designated {
                let value = self.initializer(&item.init, field_ty, name);
                cursor.advance();
                value?
            } else {
                self.fill_element(field_ty, cursor, name, item_range)?
            };
            slots[index] = Some(if path.len() > 1 {
                // The designator named a member of an anonymous member; the
                // value goes inside it, merged with whatever an earlier
                // designator put there.
                let Ty::Record(inner) = fields[index].0 else {
                    return None;
                };
                let current = slots[index].take();
                self.place_member(current, inner, &path[1..], value)
            } else {
                value
            });
            index += 1;
        }

        let mut values = Vec::with_capacity(fields.len());
        for (slot, (field_ty, _)) in slots.into_iter().zip(fields) {
            match slot {
                Some(value) => values.push(value),
                None => {
                    let zero = self.zero(field_ty, range);
                    values.push(zero);
                }
            }
        }
        Some(Expr::new(
            ExprKind::RecordLit {
                record,
                fields: values,
            },
            ty,
            range,
        ))
    }

    /// The type of the member a [member path](Sema::member_path) reaches.
    fn member_type(&self, record: RecordId, path: &[usize]) -> Ty {
        let field = &self.types().record(record).fields[path[0]];
        match (path.len(), field.ty) {
            (1, ty) => ty,
            (_, Ty::Record(inner)) => self.member_type(inner, &path[1..]),
            (_, ty) => ty,
        }
    }

    /// Puts `value` at `path` inside a value of `record`, merging it into
    /// `current` if an earlier designator already built one.
    ///
    /// This is what makes `{ .a = 1, .b = 2 }` work when `a` and `b` both live
    /// in the same anonymous member: the first designator builds the value of
    /// that member, and the second one updates it rather than replacing it.
    fn place_member(
        &mut self,
        current: Option<Expr>,
        record: RecordId,
        path: &[usize],
        value: Expr,
    ) -> Expr {
        let def = self.types().record(record);
        let kind = def.kind;
        let index = path[0];
        let field_ty = def.fields[index].ty;
        let range = value.range;
        let inner = if path.len() == 1 {
            value
        } else if let Ty::Record(sub) = field_ty {
            let existing = existing_member(current.as_ref(), index);
            self.place_member(existing, sub, &path[1..], value)
        } else {
            value
        };
        let ty = Ty::Record(record);
        if kind == RecordKind::Union {
            return Expr::new(
                ExprKind::UnionLit {
                    record,
                    index,
                    value: Box::new(inner),
                },
                ty,
                range,
            );
        }
        let mut values = match current {
            Some(Expr {
                kind: ExprKind::RecordLit { fields, .. },
                ..
            }) => fields,
            _ => {
                let tys: Vec<Ty> = self
                    .types()
                    .record(record)
                    .fields
                    .iter()
                    .map(|f| f.ty)
                    .collect();
                tys.into_iter().map(|ty| self.zero(ty, range)).collect()
            }
        };
        if let Some(slot) = values.get_mut(index) {
            *slot = inner;
        }
        Expr::new(
            ExprKind::RecordLit {
                record,
                fields: values,
            },
            ty,
            range,
        )
    }

    /// Evaluates `[k]` in a designator.
    fn designator_index(&mut self, expr: &ast::Expr) -> Option<u64> {
        let value = self.expr(expr)?;
        if !value.ty.is_integer() {
            self.error(expr.range, "array designator must have integer type");
            return None;
        }
        match self.const_eval_at(&value, "array designator")? {
            ConstValue::Int(v) if v >= 0 => u64::try_from(v).ok(),
            ConstValue::Int(_) => {
                self.error(expr.range, "array designator index is negative");
                None
            }
            ConstValue::Float(_) => None,
        }
    }

    /// `char s[N] = "…"` and its wide counterpart.
    fn string_initializer(
        &mut self,
        lit: &StrLit,
        array_ty: Ty,
        range: SourceRange,
    ) -> Option<Expr> {
        let Ty::Array(id) = array_ty else {
            return None;
        };
        let array = self.types().array_type(id);
        let wide = lit.kind == StrKind::Wide;
        let elem_ok = if wide {
            array.elem == Ty::wchar_ty()
        } else {
            matches!(array.elem, Ty::Char | Ty::SChar | Ty::UChar)
        };
        if !elem_ok {
            self.error(
                range,
                format!(
                    "cannot initialize an array of '{}' with a {} string literal",
                    self.tyname(array.elem),
                    if wide { "wide" } else { "narrow" }
                ),
            );
            return None;
        }

        let mut values: Vec<i128> = lit.values.iter().map(|v| i128::from(*v)).collect();
        values.push(0);
        if values.len() as u64 > array.len {
            // C99 6.7.8p14 lets the terminating NUL fall off the end.
            if values.len() as u64 == array.len + 1 {
                values.pop();
            } else {
                self.error(range, "initializer-string for char array is too long");
                values.truncate(array.len as usize);
            }
        }
        let target = self.target;
        let mut items: Vec<Expr> = values
            .into_iter()
            .map(|v| Expr::int(array.elem.wrap(v, &target), array.elem, range))
            .collect();
        while (items.len() as u64) < array.len {
            items.push(Expr::int(0, array.elem, range));
        }
        Some(Expr::new(ExprKind::ArrayLit(items), array_ty, range))
    }
}

/// The string literal a braced initialiser list is nothing but, if it is one.
///
/// `{ "hi" }` initialising a character array is the string initialiser with
/// braces around it (C99 6.7.8p14), not a list whose single element is a
/// `char *`.
fn braced_string<'a>(cursor: &Cursor<'a>) -> Option<&'a StrLit> {
    let item = cursor.peek()?;
    if !item.designators.is_empty() {
        return None;
    }
    match &item.init.kind {
        ast::InitializerKind::Expr(ast::Expr {
            kind: ast::ExprKind::Str(lit),
            ..
        }) => Some(lit),
        _ => None,
    }
}

/// Whether a string literal is the kind that initialises an array of `elem`.
fn fills_array(elem: Ty, lit: &StrLit) -> bool {
    if lit.kind == StrKind::Wide {
        elem == Ty::wchar_ty()
    } else {
        matches!(elem, Ty::Char | Ty::SChar | Ty::UChar)
    }
}

/// The value an aggregate literal already holds for one of its members.
fn existing_member(current: Option<&Expr>, index: usize) -> Option<Expr> {
    match &current?.kind {
        ExprKind::RecordLit { fields, .. } => fields.get(index).cloned(),
        ExprKind::UnionLit {
            index: at, value, ..
        } if *at == index => Some((**value).clone()),
        _ => None,
    }
}

/// Whether a value is an all-bits-zero constant.
fn is_zero_value(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int(v) => *v == 0,
        ExprKind::Float(v) => *v == 0.0,
        ExprKind::Zeroed => true,
        ExprKind::ArrayRepeat { value, .. } => is_zero_value(value),
        ExprKind::ArrayLit(items) => items.iter().all(is_zero_value),
        ExprKind::RecordLit { fields, .. } => fields.iter().all(is_zero_value),
        _ => false,
    }
}
