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
//! # Designator lists
//!
//! A designator may reach any number of levels down — `.a.b = 1`,
//! `.arr[2].x = 3`, `[1].y = 2` — and 6.7.8p17 then continues with the
//! subobject *after* the one it named, which may be back up several levels
//! again: `{ .i[0].p[1] = 5, 6, 7, 8 }` fills `i[0].p[1]`, `i[1].p[0]`,
//! `i[1].p[1]` and the member after `i`. That is what [`Step`] is for: the
//! position inside the element currently being filled is a path rather than an
//! index, [`Sema::advance_steps`] is the "next subobject" of 6.7.8p17, and
//! [`Sema::place_steps`] merges the value into whatever earlier designators
//! have already built there.

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
    /// The value of each plain-expression element that has already been
    /// checked, by position.
    ///
    /// 6.7.9p13 lets an element of structure or union type initialise a
    /// *subobject* of that type whole, braces or not, so filling a
    /// subaggregate has to know the element's type before it decides whether
    /// to descend into it. That means checking the expression, and the same
    /// expression is then used either for the subobject or for its first
    /// member — once, not twice, because checking one can create a temporary,
    /// a static or a diagnostic. The outer `Option` is "not looked at yet";
    /// the inner one is what the check produced.
    checked: Vec<Option<Option<Expr>>>,
}

impl<'a> Cursor<'a> {
    fn new(items: &'a [ast::InitItem]) -> Self {
        Self {
            items,
            pos: 0,
            checked: Vec::new(),
        }
    }

    fn peek(&self) -> Option<&'a ast::InitItem> {
        self.items.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }
}

/// One step from an object to one of its subobjects.
///
/// A designator list, and the position 6.7.8p17 continues from, are both a run
/// of these: `.arr[2].x` is `[Field(arr), Index(2), Field(x)]`. The steps a
/// C11 anonymous member adds are ordinary [`Step::Field`]s — the member list
/// is what makes them transparent, not the path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    /// A member of a `struct` or `union`, by index.
    Field(usize),
    /// An element of an array.
    Index(u64),
    /// GNU's `[low ... high]`, which fills every element of the range.
    Range(u64, u64),
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
                let value = self.fill(ty, &mut cursor, name, init.range, false)?;
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
            if self.types().is_vm(element) {
                self.error(
                    type_name.range,
                    "a compound literal cannot have a variable length array type",
                );
                return None;
            }
            let elem_const = elem.qualifiers.is_const;
            self.init_array_inferred(&init, element, elem_const, "compound literal")?
        } else {
            let ty = self.ty_of(&type_name.ty)?;
            // C99 6.5.2.5p1: the type name may not be a variable length array,
            // and an initialiser could not say how many elements it has.
            if self.types().is_vm(ty) {
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
                if let Some(lit) =
                    braced_string(&cursor).filter(|lit| fills_array(elem, lit, &self.target))
                {
                    let len = lit.values.len() as u64 + 1;
                    let ty = self.program.types.array(elem, len, elem_const);
                    let value = self.string_initializer(lit, ty, init.range)?;
                    cursor.advance();
                    if let Some(extra) = cursor.peek() {
                        self.error(extra.range, "excess elements in initializer");
                    }
                    return Some((ty, value));
                }
                let filled = self.fill_array(elem, None, &mut cursor, name, init.range, false)?;
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
    ///
    /// `elided` says that the braces around this aggregate were left out, so
    /// that the brace-enclosed list — and therefore the object a designator
    /// names — is an enclosing one. An element carrying a designator then ends
    /// the run and is left for the level that owns it: in
    /// `struct S { struct A a; int t; } s = { 1, .t = 5 }` the `.t` names a
    /// member of `s`, not of `s.a`.
    fn fill(
        &mut self,
        ty: Ty,
        cursor: &mut Cursor,
        name: &str,
        range: SourceRange,
        elided: bool,
    ) -> Option<Expr> {
        match ty {
            Ty::Array(id) => {
                let array = self.types().array_type(id);
                // `char s[4] = { "abc" }` — the braces around the string are
                // C's, not a one-element list of characters.
                if let Some(lit) =
                    braced_string(cursor).filter(|lit| fills_array(array.elem, lit, &self.target))
                {
                    let value = self.string_initializer(lit, ty, range);
                    cursor.advance();
                    return value;
                }
                let (values, len) =
                    self.fill_array(array.elem, Some(array.len), cursor, name, range, elided)?;
                Some(self.assemble_array(values, array.elem, len, range))
            }
            Ty::Record(id) => self.fill_record(id, cursor, name, range, elided),
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
                let value = self.checked_initializer(cursor, &item.init, ty, name);
                cursor.advance();
                value
            }
        }
    }

    /// [`Sema::initializer`], with a plain expression going through the
    /// cursor's cache.
    ///
    /// See [`Cursor::checked`]: an expression may be checked once to find out
    /// whether it initialises a subaggregate whole and then used for one of
    /// that subaggregate's members, and checking it twice would duplicate
    /// whatever the check created.
    fn checked_initializer(
        &mut self,
        cursor: &mut Cursor,
        init: &ast::Initializer,
        ty: Ty,
        name: &str,
    ) -> Option<Expr> {
        // An array is initialised by a string literal or by a list, never by
        // an expression whose value could be cached.
        if ty.is_array() {
            return self.initializer(init, ty, name);
        }
        let ast::InitializerKind::Expr(expr) = &init.kind else {
            return self.initializer(init, ty, name);
        };
        let value = self.check_item(cursor, expr)?;
        Some(self.convert_for(value, ty, ConvContext::Init(name.to_owned())))
    }

    /// Checks the plain expression the cursor is on, at most once.
    fn check_item(&mut self, cursor: &mut Cursor, expr: &ast::Expr) -> Option<Expr> {
        let pos = cursor.pos;
        if let Some(Some(cached)) = cursor.checked.get(pos) {
            return cached.clone();
        }
        let value = self.expr(expr);
        if cursor.checked.len() <= pos {
            cursor.checked.resize(pos + 1, None);
        }
        cursor.checked[pos] = Some(value.clone());
        value
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
        // 6.7.9p13: "The initializer for a structure or union object … shall
        // be … a single expression that has compatible structure or union
        // type", and that holds for a *subobject* as much as for the object
        // itself — `struct A { int z; struct B b; } a = { 2, b }` gives `a.b`
        // the whole of `b`. Only when the element is not of the subaggregate's
        // own type do the elided braces of p9 apply and the element go to the
        // first member instead, so the element's type has to be known first.
        // `Cursor::checked` is what keeps that from checking it twice.
        if !braced
            && !whole
            && ty.is_record()
            && let ast::InitializerKind::Expr(expr) = &item.init.kind
        {
            let value = self.check_item(cursor, expr);
            if value
                .as_ref()
                .is_some_and(|value| value.ty.is_record() && self.compatible(value.ty, ty))
            {
                cursor.advance();
                let value = value.expect("checked just above");
                return Some(self.convert_for(value, ty, ConvContext::Init(name.to_owned())));
            }
        }
        if braced || string || whole || !(ty.is_array() || ty.is_record()) {
            let value = self.checked_initializer(cursor, &item.init, ty, name);
            cursor.advance();
            return value;
        }
        self.fill(ty, cursor, name, range, true)
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
        elided: bool,
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
        // Where inside the element at `index` the last designated initialiser
        // landed; empty when it filled the element whole. 6.7.8p17 continues
        // from the subobject after that one, which is what makes
        // `{ [1].y = 7, 8 }` give the `8` to whatever follows `[1].y`.
        let mut sub: Vec<Step> = Vec::new();
        while let Some(item) = cursor.peek() {
            if item.designators.is_empty() {
                // Running out of room ends the run — but only for an element
                // that takes the next position; a designator may point back
                // into the array from anywhere.
                if sub.is_empty() && len.is_some_and(|len| index >= len) {
                    break;
                }
            } else if elided {
                // These braces were left out, so the designator belongs to the
                // list above and the run of elided elements ends here.
                break;
            }
            let item_range = item.range;
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
                                item_range,
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
                // `[1].y = 2` — the rest of the list reaches into the element.
                sub = if item.designators.len() == 1 {
                    Vec::new()
                } else {
                    match self.nested_steps(elem, &item.designators[1..]) {
                        Some(steps) => steps,
                        None => {
                            cursor.advance();
                            continue;
                        }
                    }
                };
            }
            let last = upto.unwrap_or(index);
            if len.is_some_and(|len| last >= len) || last >= MAX_INIT_ELEMENTS {
                self.error(item_range, "array designator index is out of bounds");
                cursor.advance();
                continue;
            }
            let slot = last as usize;
            if slot >= slots.len() {
                slots.resize_with(slot + 1, || None);
            }
            if designated || !sub.is_empty() {
                let value = self.fill_at(elem, &mut sub, item, name)?;
                cursor.advance();
                // A range designator writes one checked value into every
                // element it covers; the value is a constant in every use that
                // matters, and C leaves the number of evaluations unspecified.
                for slot in index..=last {
                    let slot = slot as usize;
                    let placed = if sub.is_empty() {
                        value.clone()
                    } else {
                        let current = slots[slot].take();
                        self.place_steps(current, elem, &sub, value.clone())
                    };
                    slots[slot] = Some(placed);
                }
            } else {
                slots[slot] = Some(self.fill_element(elem, cursor, name, item_range)?);
            }
            match self.advance_steps(elem, &sub) {
                Some(next) => {
                    index = last;
                    sub = next;
                }
                None => {
                    index = last + 1;
                    sub.clear();
                }
            }
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
        elided: bool,
    ) -> Option<Expr> {
        let ty = Ty::Record(record);
        let def = self.types().record(record);
        let kind = def.kind;
        // A flexible array member has no elements, so there is nothing an
        // initialiser could put in it; the flag rides along with the type.
        let fields: Vec<(Ty, bool)> = def.fields.iter().map(|f| (f.ty, f.flexible)).collect();
        if fields.is_empty() {
            return Some(Expr::new(ExprKind::Zeroed, ty, range));
        }

        let mut slots: Vec<Option<Expr>> = (0..fields.len()).map(|_| None).collect();
        let mut index = 0usize;
        // The position inside the member at `index`; see [`Sema::fill_array`].
        let mut sub: Vec<Step> = Vec::new();
        // For a union, the member the initialiser has given a value to. A
        // union initialiser names exactly one (6.7.8p16), so a second element
        // that takes the next position is an excess one rather than the next
        // member — but one that *continues* inside the member already named,
        // as `{ .a.x = 1, 2 }` does, is not.
        let mut live: Option<usize> = None;
        while let Some(item) = cursor.peek() {
            if item.designators.is_empty() {
                let exhausted =
                    index >= fields.len() || (kind == RecordKind::Union && live.is_some());
                if sub.is_empty() && exhausted {
                    break;
                }
            } else if elided {
                // These braces were left out, so the designator belongs to the
                // list above; see [`Sema::fill`].
                break;
            }
            let item_range = item.range;
            let designated = !item.designators.is_empty();
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
                        // A designator may name a member of an anonymous
                        // member, which is reached through it, and may carry
                        // on from there: `.a.b = 1`.
                        let member = self.member_type(record, &found);
                        let mut steps: Vec<Step> =
                            found[1..].iter().map(|i| Step::Field(*i)).collect();
                        if item.designators.len() > 1 {
                            let Some(rest) = self.nested_steps(member, &item.designators[1..])
                            else {
                                cursor.advance();
                                continue;
                            };
                            steps.extend(rest);
                        }
                        sub = steps;
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
            }
            // C99 6.7.2.1p18 says a flexible array member cannot be
            // initialized, and GCC agrees.
            if fields.get(index).is_some_and(|(_, flexible)| *flexible) {
                self.error(
                    item_range,
                    "a flexible array member cannot be initialized; allocate the object with \
                     room for the elements and fill them in",
                );
                cursor.advance();
                index += 1;
                sub.clear();
                continue;
            }
            let field_ty = fields[index].0;
            let value = if designated || !sub.is_empty() {
                let value = self.fill_at(field_ty, &mut sub, item, name)?;
                cursor.advance();
                if sub.is_empty() {
                    value
                } else {
                    // The value went below the member; merge it with whatever
                    // an earlier designator put there.
                    let current = slots[index].take();
                    self.place_steps(current, field_ty, &sub, value)
                }
            } else {
                self.fill_element(field_ty, cursor, name, item_range)?
            };
            slots[index] = Some(value);
            live = Some(index);
            match self.advance_steps(field_ty, &sub) {
                Some(next) => sub = next,
                None => {
                    index += 1;
                    sub.clear();
                }
            }
        }

        if kind == RecordKind::Union {
            // A union value is one member's, and which one is part of the
            // value: whichever the initialiser last named.
            let index = live.unwrap_or(0);
            let Some(value) = slots.into_iter().nth(index).flatten() else {
                return Some(Expr::new(ExprKind::Zeroed, ty, range));
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

    /// Fills the subobject `steps` reaches inside an object of type `top` from
    /// one initialiser element.
    ///
    /// The braces around a subaggregate may be left out (6.7.8p20), which
    /// below a designator means the element may land several levels further
    /// down than the designator reached: in `{ .arr = 1, 2, 3 }` the `1` goes
    /// to `arr[0]` and the rest carry on from there. `steps` grows by the
    /// levels that were descended, so that the caller places the value where
    /// it really went and continues from *that* position.
    ///
    /// The expression is evaluated once, before the descent, because how far
    /// to descend is exactly the question of whether its own type is the one
    /// being initialised: `.a = other` gives the whole member when `other` is
    /// a `struct A`, and `a.x` the value when it is an `int`.
    fn fill_at(
        &mut self,
        top: Ty,
        steps: &mut Vec<Step>,
        item: &ast::InitItem,
        name: &str,
    ) -> Option<Expr> {
        let expr = match &item.init.kind {
            ast::InitializerKind::List(_) => {
                let target = self.type_at(top, steps);
                return self.initializer(&item.init, target, name);
            }
            ast::InitializerKind::Expr(expr) => expr,
        };
        // A string literal initialises a character array whole, however many
        // levels of array lie between here and one.
        if let ast::ExprKind::Str(lit) = &expr.kind {
            while let Ty::Array(id) = self.type_at(top, steps) {
                let array = self.types().array_type(id);
                if fills_array(array.elem, lit, &self.target) {
                    let target = self.type_at(top, steps);
                    return self.string_initializer(lit, target, expr.range);
                }
                if !array.elem.is_array() {
                    break;
                }
                steps.push(Step::Index(0));
            }
        }
        let value = self.expr(expr)?;
        loop {
            let target = self.type_at(top, steps);
            if value.ty == target {
                break;
            }
            match self.first_step(target) {
                Some(step) => steps.push(step),
                None => break,
            }
        }
        let target = self.type_at(top, steps);
        Some(self.convert_for(value, target, ConvContext::Init(name.to_owned())))
    }

    /// Resolves the designators after the first against the subobject that
    /// first one reached.
    ///
    /// The first designator of a list names an element of the aggregate being
    /// filled, and is resolved by the caller, which is the only one that knows
    /// whether that aggregate has a length yet; everything after it walks down
    /// from there.
    fn nested_steps(&mut self, ty: Ty, designators: &[ast::Designator]) -> Option<Vec<Step>> {
        let mut steps = Vec::new();
        let mut current = ty;
        for designator in designators {
            match designator {
                ast::Designator::Field(field) => {
                    let Ty::Record(record) = current else {
                        self.error(
                            field.range,
                            format!(
                                "a field designator cannot initialize a subobject of type '{}'",
                                self.tyname(current)
                            ),
                        );
                        return None;
                    };
                    let Some(found) = self.member_path(record, &field.name) else {
                        self.error(
                            field.range,
                            format!(
                                "no member named '{}' in '{}'",
                                field.name,
                                self.tyname(current)
                            ),
                        );
                        return None;
                    };
                    current = self.member_type(record, &found);
                    steps.extend(found.into_iter().map(Step::Field));
                }
                ast::Designator::Index(expr) | ast::Designator::Range(expr, _) => {
                    let Ty::Array(id) = current else {
                        self.error(
                            expr.range,
                            format!(
                                "an array designator cannot initialize a subobject of type '{}'",
                                self.tyname(current)
                            ),
                        );
                        return None;
                    };
                    let step = match designator {
                        ast::Designator::Range(low, high) => {
                            let low = self.designator_index(low)?;
                            let high = self.designator_index(high)?;
                            if high < low {
                                self.error(
                                    expr.range,
                                    "empty range designator: the last index is below the first",
                                );
                                return None;
                            }
                            Step::Range(low, high)
                        }
                        _ => Step::Index(self.designator_index(expr)?),
                    };
                    let array = self.types().array_type(id);
                    let highest = match step {
                        Step::Index(index) => index,
                        Step::Range(_, high) => high,
                        Step::Field(_) => unreachable!("built just above"),
                    };
                    if highest >= array.len || highest >= MAX_INIT_ELEMENTS {
                        self.error(expr.range, "array designator index is out of bounds");
                        return None;
                    }
                    current = array.elem;
                    steps.push(step);
                }
            }
        }
        Some(steps)
    }

    /// The type of the subobject `steps` reaches inside an object of type `ty`.
    fn type_at(&self, ty: Ty, steps: &[Step]) -> Ty {
        let mut current = ty;
        for step in steps {
            current = match (current, step) {
                (Ty::Record(record), Step::Field(index)) => {
                    match self.types().record(record).fields.get(*index) {
                        Some(field) => field.ty,
                        None => return current,
                    }
                }
                (Ty::Array(id), Step::Index(_) | Step::Range(..)) => {
                    self.types().array_type(id).elem
                }
                _ => return current,
            };
        }
        current
    }

    /// The first subobject of an aggregate: element zero, or member zero.
    fn first_step(&self, ty: Ty) -> Option<Step> {
        match ty {
            Ty::Array(id) => (self.types().array_type(id).len > 0).then_some(Step::Index(0)),
            Ty::Record(id) => {
                let def = self.types().record(id);
                (!def.fields.is_empty() && !def.fields[0].flexible).then_some(Step::Field(0))
            }
            _ => None,
        }
    }

    /// The subobject after `steps` inside an object of type `ty`, which is
    /// 6.7.8p17's "next subobject".
    ///
    /// `None` says there is nothing after it, so that the caller moves on to
    /// the next element of the level it owns. Running off the end of a nested
    /// aggregate pops back up, which is how `{ .i[0].p[1] = 5, 6, 7, 8 }`
    /// carries on into `i[1]` and then out of `i` altogether.
    fn advance_steps(&self, ty: Ty, steps: &[Step]) -> Option<Vec<Step>> {
        let (last, prefix) = steps.split_last()?;
        let parent = self.type_at(ty, prefix);
        let next = match (parent, last) {
            (Ty::Record(record), Step::Field(index)) => {
                let def = self.types().record(record);
                let after = index + 1;
                match def.fields.get(after) {
                    // A union holds one member at a time, so nothing follows
                    // the one an initialiser reached; a flexible array member
                    // cannot be initialised, so nothing follows the member in
                    // front of it either.
                    Some(field) if def.kind != RecordKind::Union && !field.flexible => {
                        Some(Step::Field(after))
                    }
                    _ => None,
                }
            }
            (Ty::Array(id), Step::Index(_) | Step::Range(..)) => {
                let reached = match last {
                    Step::Index(index) => *index,
                    Step::Range(_, high) => *high,
                    Step::Field(_) => unreachable!("matched above"),
                };
                let array = self.types().array_type(id);
                (reached + 1 < array.len).then_some(Step::Index(reached + 1))
            }
            _ => None,
        };
        match next {
            Some(step) => {
                let mut out = prefix.to_vec();
                out.push(step);
                Some(out)
            }
            None => self.advance_steps(ty, prefix),
        }
    }

    /// Puts `value` at `steps` inside an object of type `ty`, merging it into
    /// `current` if an earlier designator already built one.
    ///
    /// This is what makes `{ .a.x = 1, .a.y = 2 }` work — and, with the same
    /// code, `{ .a = 1, .a.y = 2 }` and a designator into an anonymous member:
    /// the first element builds the value of `a`, and the second updates it
    /// rather than replacing it.
    fn place_steps(&mut self, current: Option<Expr>, ty: Ty, steps: &[Step], value: Expr) -> Expr {
        let Some(step) = steps.first() else {
            return value;
        };
        let range = value.range;
        match (ty, *step) {
            (Ty::Record(record), Step::Field(index)) => {
                let def = self.types().record(record);
                let kind = def.kind;
                let Some(field_ty) = def.fields.get(index).map(|field| field.ty) else {
                    return value;
                };
                let existing = existing_member(current.as_ref(), index);
                let inner = self.place_steps(existing, field_ty, &steps[1..], value);
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
            (Ty::Array(id), Step::Index(_) | Step::Range(..)) => {
                let array = self.types().array_type(id);
                if array.len > MAX_INIT_ELEMENTS {
                    self.error(
                        range,
                        format!(
                            "an array of more than {MAX_INIT_ELEMENTS} elements cannot be \
                             initialized element by element"
                        ),
                    );
                    return value;
                }
                let (low, high) = match *step {
                    Step::Index(index) => (index, index),
                    Step::Range(low, high) => (low, high),
                    Step::Field(_) => unreachable!("matched above"),
                };
                let mut items = self.array_items(current, array.elem, array.len, range);
                for index in low..=high.min(array.len.saturating_sub(1)) {
                    let existing = items.get(index as usize).cloned();
                    let inner = self.place_steps(existing, array.elem, &steps[1..], value.clone());
                    if let Some(slot) = items.get_mut(index as usize) {
                        *slot = inner;
                    }
                }
                Expr::new(ExprKind::ArrayLit(items), ty, range)
            }
            _ => value,
        }
    }

    /// The elements an array value already holds, one slot per element.
    fn array_items(
        &mut self,
        current: Option<Expr>,
        elem: Ty,
        len: u64,
        range: SourceRange,
    ) -> Vec<Expr> {
        match current {
            Some(Expr {
                kind: ExprKind::ArrayLit(items),
                ..
            }) if items.len() as u64 == len => items,
            Some(Expr {
                kind: ExprKind::ArrayRepeat { value, len: repeat },
                ..
            }) if repeat == len => (0..len).map(|_| (*value).clone()).collect(),
            _ => (0..len).map(|_| self.zero(elem, range)).collect(),
        }
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
            // Neither can arrive: the designator was checked to be an integer.
            ConstValue::Float(_) | ConstValue::Complex(..) => None,
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
        if !fills_array(array.elem, lit, &self.target) {
            self.error(
                range,
                format!(
                    "cannot initialize an array of '{}' with a {} string literal",
                    self.tyname(array.elem),
                    literal_kind(lit.kind)
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
                // Anything more is a constraint violation (6.7.8p2: no
                // initializer may provide a value for something outside the
                // object), and a strict entry point says so. GCC's is a
                // warning and the extra characters are dropped, so a GNU
                // dialect drops them too — `execute/pr86714` is `const char
                // a[2][3] = { "1234", "xyz" }`, and its whole point is that
                // the excess is not part of the value.
                if !self.gating.dialect.is_gnu() {
                    self.error(range, "initializer-string for char array is too long");
                }
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
///
/// C11 6.7.9p14–15: a narrow or `u8"…"` literal fills an array of any
/// character type, and each of the other three fills an array of exactly the
/// type its own prefix names.
fn fills_array(elem: Ty, lit: &StrLit, target: &crate::TargetModel) -> bool {
    match lit.kind {
        StrKind::Narrow | StrKind::Utf8 => matches!(elem, Ty::Char | Ty::SChar | Ty::UChar),
        StrKind::Utf16 => elem == Ty::char16_ty(),
        StrKind::Utf32 => elem == Ty::char32_ty(),
        StrKind::Wide => elem == Ty::wchar_ty(target),
    }
}

/// How a diagnostic names a string literal of this kind.
fn literal_kind(kind: StrKind) -> &'static str {
    match kind {
        StrKind::Narrow => "narrow",
        StrKind::Utf8 => "'u8'",
        StrKind::Utf16 => "'u'",
        StrKind::Utf32 => "'U'",
        StrKind::Wide => "wide",
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
