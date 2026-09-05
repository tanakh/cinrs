//! Declarations: objects, `typedef`s, functions and the `extern` block.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, Expr, ExprKind, FuncId, Function, ObjectId, Place, PlaceKind, Signature, StaticVar, Stmt,
    Storage, Ty, TypedefItem,
};

use super::{Entry, Sema, TypedefEntry};

impl Sema {
    // -- static assertions --------------------------------------------------

    /// Checks a `_Static_assert` declaration, which generates nothing at all.
    ///
    /// It is the same check wherever the declaration was written — at file
    /// scope, inside a block or among a record's members — because a constant
    /// expression means the same thing in all three.
    pub(super) fn static_assert(&mut self, assert: &ast::StaticAssert) {
        let Some(value) = self.expr(&assert.cond) else {
            return;
        };
        if !value.ty.is_integer() {
            self.error(
                assert.cond.range,
                format!(
                    "the controlling expression of a static assertion must have an integer \
                     type, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return;
        }
        let what = "the controlling expression of a static assertion";
        let Some(constant) = self.const_eval_at(&value, what) else {
            return;
        };
        if super::is_true(constant) {
            return;
        }
        let message = match &assert.message {
            Some(text) => format!("static assertion failed: {text}"),
            None => "static assertion failed".to_owned(),
        };
        self.error(assert.range, message);
    }

    // -- top level ----------------------------------------------------------

    pub(super) fn file_scope_decl(&mut self, decl: &ast::Decl) {
        if decl.declarators.is_empty() {
            // `struct S { … };` and friends: a tag definition with nothing
            // declared. Resolving the specifier is what defines the tag.
            let _ = self.ty_of(&decl.specifiers.base);
            return;
        }
        for declarator in &decl.declarators {
            self.declarator(decl, declarator, true);
        }
    }

    /// Handles one declarator of a declaration, at file or block scope.
    ///
    /// Returns the statements the declarator contributes to the enclosing
    /// block (empty at file scope, for `typedef`s and for `static` locals).
    pub(super) fn declarator(
        &mut self,
        decl: &ast::Decl,
        declarator: &ast::InitDeclarator,
        file_scope: bool,
    ) -> Vec<Stmt> {
        let Some(name) = &declarator.name else {
            // Nothing is declared, but the specifier may still define a tag.
            let _ = self.ty_of(&decl.specifiers.base);
            return Vec::new();
        };

        if decl.specifiers.is_typedef() {
            self.declare_typedef(name, &declarator.ty, declarator.init.as_ref());
            return Vec::new();
        }

        let storage = decl.specifiers.storage.as_ref().map(|s| s.node);
        if let ast::TypeKind::Function(func) = &declarator.ty.kind {
            let mut attrs = declarator.attrs.clone();
            attrs.merge(decl.specifiers.attrs.clone());
            self.declare_function(
                decl,
                name,
                func,
                declarator.ty.range,
                &attrs,
                declarator.asm_label.as_ref(),
                None,
            );
            return Vec::new();
        }

        // Everything below declares an *object*, which none of the function
        // specifiers apply to.
        if let Some(range) = decl.specifiers.noreturn {
            self.error(range, "'_Noreturn' is only allowed on a function");
        }
        if let Some(alignment) = &decl.specifiers.alignas {
            self.error(
                alignment.range,
                "an alignment specifier on an object is not supported yet; '_Alignas' and \
                 '__attribute__((aligned))' are honoured on the members of a struct or \
                 union, where the generated Rust type can carry the alignment",
            );
        }
        let mut attrs = declarator.attrs.clone();
        attrs.merge(decl.specifiers.attrs.clone());
        if let Some(range) = attrs.packed {
            self.error(range, "'packed' is only meaningful on a record or a member");
        }

        if storage == Some(ast::StorageClass::ThreadLocal) {
            let range = decl
                .specifiers
                .storage
                .as_ref()
                .map_or(declarator.range, |s| s.range);
            self.error(
                range,
                "'_Thread_local' is not supported yet; Rust's own `#[thread_local]` is \
                 unstable",
            );
            return Vec::new();
        }
        if storage == Some(ast::StorageClass::Extern) {
            self.declare_extern_object(name, declarator);
            if let Some(Entry::Object(id)) = self.lookup(&name.name).cloned() {
                self.apply_object_attributes(id, &attrs, declarator);
            }
            return Vec::new();
        }

        // `T x[] = …` has no size of its own; the initialiser gives it one,
        // which means the type and the initialiser have to be built together.
        // A declaration that did not check out still declares its name, with a
        // type that silences every later complaint about it.
        let (ty, init) = self
            .typed_initializer(declarator, &name.name)
            .unwrap_or((Ty::Error, None));

        if storage == Some(ast::StorageClass::Constexpr) {
            self.declare_constexpr(name, ty, init, declarator);
            return Vec::new();
        }
        let is_const = declarator.ty.qualifiers.is_const;
        let is_static = storage == Some(ast::StorageClass::Static);

        if file_scope || is_static {
            // An object with static storage duration outlives every argument
            // list there could be, and Rust's `VaList` says so with a lifetime
            // no item could name.
            let ty = if self.reject_va_list(ty, declarator.range) {
                Ty::Error
            } else {
                ty
            };
            self.declare_static_object(name, ty, is_const, is_static, file_scope, declarator, init);
            if let Some(Entry::Object(id)) = self.lookup(&name.name).cloned() {
                self.apply_object_attributes(id, &attrs, declarator);
            }
            return Vec::new();
        }

        if let Some(label) = &declarator.asm_label {
            self.error(
                label.range,
                "an 'asm' label on a local variable is not supported; it names a register \
                 or a symbol, and neither has a place in the generated Rust",
            );
        }
        if let Some(section) = &attrs.section {
            self.error(
                section.range,
                "'section' is only meaningful on a function or an object with static \
                 storage duration",
            );
        }
        self.check_redefinition(name);
        let id = self.new_object(&name.name, ty, Storage::Automatic, is_const, name.range);
        self.insert(&name.name, Entry::Object(id));
        if ty.is_error() {
            return Vec::new();
        }
        // A `va_list` has no zero value: it starts out as a copy of the list
        // the function was called with, which is also what `va_start` puts
        // back into it.
        if ty.is_va_list() {
            self.gate_va_list(declarator.range);
        }
        if ty.is_va_list() && init.is_none() {
            let Some(init) = self.va_list_init(declarator.range) else {
                // The name stays declared, but as something already reported,
                // so that using it adds nothing.
                self.program.objects[id.0 as usize].ty = Ty::Error;
                return Vec::new();
            };
            return vec![Stmt::Let {
                object: id,
                init,
                explicit: true,
            }];
        }
        let explicit = init.is_some();
        let init = match init {
            Some(init) => init,
            None => self.zero(ty, declarator.range),
        };
        vec![Stmt::Let {
            object: id,
            init,
            explicit,
        }]
    }

    /// Puts what `__asm__("symbol")` and `__attribute__((section("…")))`
    /// asked for onto an object with static storage duration.
    fn apply_object_attributes(
        &mut self,
        id: ObjectId,
        attrs: &ast::Attributes,
        declarator: &ast::InitDeclarator,
    ) {
        if let Some(label) = &declarator.asm_label {
            self.program.objects[id.0 as usize].asm_label = Some(label.node.clone());
        }
        if let Some(section) = &attrs.section {
            self.program.objects[id.0 as usize].section = Some(section.node.clone());
        }
    }

    /// Declares a C23 `constexpr` object.
    ///
    /// The object becomes a *constant* rather than storage: every use of the
    /// name is folded to its value, which is what makes it usable as an array
    /// bound or a `case` label. The cost — and the reason only arithmetic
    /// objects are accepted — is that there is nothing to take the address of.
    fn declare_constexpr(
        &mut self,
        name: &ast::Ident,
        ty: Ty,
        init: Option<Expr>,
        declarator: &ast::InitDeclarator,
    ) {
        if ty.is_error() {
            return;
        }
        let Some(init) = init else {
            self.error(
                declarator.range,
                format!(
                    "'{}' is declared 'constexpr' and needs an initializer",
                    name.name
                ),
            );
            return;
        };
        if !ty.is_arithmetic() {
            self.error(
                declarator.range,
                format!(
                    "a 'constexpr' object of type '{}' is not supported yet; only the \
                     arithmetic types are",
                    self.tyname(ty)
                ),
            );
            return;
        }
        let Some(value) = self.const_eval_at(&init, "the initializer of a 'constexpr' object")
        else {
            return;
        };
        self.check_redefinition(name);
        self.insert(
            &name.name,
            Entry::Constant {
                value,
                ty,
                range: name.range,
            },
        );
    }

    /// Resolves a declarator's type and its initialiser together.
    ///
    /// They cannot be separated: `char s[] = "hi"` takes the array's length
    /// from the initialiser, and the initialiser needs the element type.
    fn typed_initializer(
        &mut self,
        declarator: &ast::InitDeclarator,
        name: &str,
    ) -> Option<(Ty, Option<Expr>)> {
        // C23's `auto x = e;`: the type is whatever the initialiser has after
        // the lvalue conversion, which is exactly what checking it gives.
        if matches!(declarator.ty.kind, ast::TypeKind::Auto) {
            let Some(init) = &declarator.init else {
                self.error(
                    declarator.range,
                    format!("'{name}' is declared 'auto' and needs an initializer"),
                );
                return None;
            };
            let ast::InitializerKind::Expr(expr) = &init.kind else {
                self.error(
                    init.range,
                    format!("the type of '{name}' cannot be inferred from a braced initializer"),
                );
                return None;
            };
            let value = self.expr(expr)?;
            if value.ty.is_void() || value.ty.is_error() {
                self.error(
                    expr.range,
                    format!(
                        "the type of '{name}' cannot be inferred from an expression of type \
                         '{}'",
                        self.tyname(value.ty)
                    ),
                );
                return None;
            }
            return Some((value.ty, Some(value)));
        }

        if let ast::TypeKind::Array {
            elem,
            size: ast::ArraySize::Unspecified,
            ..
        } = &declarator.ty.kind
        {
            let element = self.ty_of(elem)?;
            let elem_const = elem.qualifiers.is_const;
            let Some(init) = &declarator.init else {
                self.error(
                    declarator.range,
                    format!(
                        "definition of variable '{name}' with array type needs an explicit \
                         size or an initializer"
                    ),
                );
                return None;
            };
            let (ty, expr) = self.init_array_inferred(init, element, elem_const, name)?;
            return Some((ty, Some(expr)));
        }

        let ty = self.object_ty_of(&declarator.ty, name)?;
        let init = match &declarator.init {
            Some(init) => Some(self.initializer(init, ty, name)?),
            None => None,
        };
        Some((ty, init))
    }

    /// Declares a file-scope object or a function-local `static`.
    #[allow(clippy::too_many_arguments)]
    fn declare_static_object(
        &mut self,
        name: &ast::Ident,
        ty: Ty,
        is_const: bool,
        is_static: bool,
        file_scope: bool,
        declarator: &ast::InitDeclarator,
        init: Option<Expr>,
    ) {
        // C99 6.9.2: a file-scope declaration without an initialiser is a
        // *tentative* definition, and repeating one — or completing it with an
        // initialiser later — is perfectly ordinary C.
        if file_scope
            && let Some(Entry::Object(existing)) = self.declared_here(&name.name).cloned()
            && self.program.object(existing).ty == ty
        {
            self.complete_tentative_definition(name, existing, declarator, init, !is_static);
            return;
        }

        self.check_redefinition(name);
        if ty.is_error() {
            // The declaration was already reported; the name is registered so
            // that using it does not produce a second diagnostic, but there is
            // nothing to generate an item from.
            let id = self.new_object(&name.name, ty, Storage::Automatic, is_const, name.range);
            self.insert(&name.name, Entry::Object(id));
            return;
        }
        let base = if file_scope {
            name.name.clone()
        } else {
            // Two functions may each have a `static` under the same name, and
            // so may two `c99!` blocks in one Rust module.
            format!(
                "__cinrs_static_{:08x}_{}_{}",
                self.program.unit_id as u32, self.func_name, name.name
            )
        };
        let item_name = self.reserve_item_name(&base);
        let id = self.new_object(
            &name.name,
            ty,
            Storage::Static {
                item_name,
                exported: file_scope && !is_static,
            },
            is_const,
            name.range,
        );
        self.insert(&name.name, Entry::Object(id));

        // C zero-initialises static storage; an initialiser, if written, must
        // be a constant expression.
        let init = match init {
            Some(init) => self
                .static_init(init, "initializer")
                .unwrap_or_else(|| self.zero(ty, declarator.range)),
            None => self.zero(ty, declarator.range),
        };
        if declarator.init.is_some() {
            self.initialized.insert(id);
        }
        self.program.statics.push(StaticVar { object: id, init });
    }

    /// Handles a repeated file-scope declaration of the same object.
    ///
    /// `int n; int n = 1;` declares one object twice and initialises it once,
    /// which C allows; a second initialiser does not.
    fn complete_tentative_definition(
        &mut self,
        name: &ast::Ident,
        id: ObjectId,
        declarator: &ast::InitDeclarator,
        init: Option<Expr>,
        defines: bool,
    ) {
        // C99 6.9.2p2: a file-scope declaration with no storage-class
        // specifier is a definition of the object — a *tentative* one when it
        // has no initialiser, which the end of the translation unit turns into
        // a definition with a zero initialiser. An earlier `extern` only said
        // that the name has external linkage; it does not stop this unit
        // defining it, so `extern int x; int x;` defines `x` here and the
        // declaration has to stop being an external one. `static` is the
        // storage class that does *not* define anything on its own, and it is
        // the only one that reaches here.
        if defines {
            self.define_here(id, declarator);
        }
        let Some(init) = init else {
            return;
        };
        if !self.initialized.insert(id) {
            let previous = self.program.object(id).range;
            self.error_note(
                name.range,
                format!("redefinition of '{}'", name.name),
                previous,
                format!("previous definition of '{}' is", name.name),
            );
            return;
        }
        let ty = self.program.object(id).ty;
        let value = self
            .static_init(init, "initializer")
            .unwrap_or_else(|| self.zero(ty, declarator.range));
        if let Some(entry) = self.program.statics.iter_mut().find(|s| s.object == id) {
            entry.init = value;
        }
    }

    /// Turns an object this unit only *declared* into one it defines.
    ///
    /// `extern int x;` puts `x` in the generated `extern` block, where the
    /// linker is expected to find it elsewhere. A later declaration of the
    /// same name without a storage class is a definition (C99 6.9.2p2), so the
    /// object moves out of the `extern` block and becomes a `static mut` item
    /// with the zero C gives an object with static storage duration; an
    /// initialiser, if the declaration wrote one, replaces that zero
    /// afterwards. Doing nothing at all is what left `extern int x; int x;`
    /// with an undefined symbol at link time.
    fn define_here(&mut self, id: ObjectId, declarator: &ast::InitDeclarator) {
        if !matches!(self.program.object(id).storage, Storage::Extern { .. }) {
            return;
        }
        let ty = self.program.object(id).ty;
        let name = self.program.object(id).name.clone();
        let item_name = self.reserve_item_name(&name);
        self.program.objects[id.0 as usize].storage = Storage::Static {
            item_name,
            exported: true,
        };
        self.program.externs.retain(|e| *e != id);
        let init = self.zero(ty, declarator.range);
        self.program.statics.push(StaticVar { object: id, init });
    }

    /// Declares an object defined outside the translation unit.
    fn declare_extern_object(&mut self, name: &ast::Ident, declarator: &ast::InitDeclarator) {
        let Some(ty) = self.object_ty_of(&declarator.ty, &name.name) else {
            return;
        };
        if let Some(init) = &declarator.init {
            self.error(
                init.range,
                format!(
                    "'{}' is declared 'extern' and cannot have an initializer here",
                    name.name
                ),
            );
        }
        if let Some(Entry::Object(existing)) = self.lookup(&name.name).cloned() {
            if self.program.object(existing).ty == ty {
                self.insert(&name.name, Entry::Object(existing));
                return;
            }
            let previous = self.program.object(existing).range;
            self.error_note(
                name.range,
                format!("redeclaration of '{}' with a different type", name.name),
                previous,
                format!("previous declaration of '{}' is", name.name),
            );
            return;
        }
        let id = self.new_object(
            &name.name,
            ty,
            Storage::Extern {
                item_name: name.name.clone(),
            },
            declarator.ty.qualifiers.is_const,
            name.range,
        );
        self.program.externs.push(id);
        // An `extern` declaration names an object with external linkage
        // wherever it is written, so the name belongs in the file scope.
        self.insert_at_file_scope(&name.name, Entry::Object(id));
        self.insert(&name.name, Entry::Object(id));
    }

    // -- typedef ------------------------------------------------------------

    fn declare_typedef(
        &mut self,
        name: &ast::Ident,
        ty: &ast::Type,
        init: Option<&ast::Initializer>,
    ) {
        if let Some(init) = init {
            self.error(init.range, "a 'typedef' cannot have an initializer");
        }
        let file_scope = self.at_file_scope();
        let resolved = match self.resolve_ty(ty) {
            Ok(ty) => Ok(ty),
            Err(err) if err.message.is_empty() => return,
            Err(err) => Err(err.message),
        };
        let already = self.declared_here(&name.name).is_some();
        if already {
            // Repeating a `typedef` is a C11 relaxation that GCC accepts in
            // C99 mode too, so only a change of meaning is worth reporting.
            let same = matches!(
                self.declared_here(&name.name),
                Some(Entry::Typedef(entry)) if entry.resolved == resolved
            );
            if !same {
                self.check_redefinition(name);
            }
        }
        self.insert(
            &name.name,
            Entry::Typedef(TypedefEntry {
                resolved: resolved.clone(),
                range: name.range,
            }),
        );

        // Only a file-scope `typedef` becomes an item Rust code can use; one
        // inside a block is resolved and forgotten, exactly as C does.
        if !file_scope || already {
            return;
        }
        let Ok(resolved) = resolved else { return };
        if self.claim_anonymous_tag(resolved, &name.name) {
            return;
        }
        // `typedef struct Point { … } Point;` is the commonest idiom in C, and
        // the tag already generated a Rust type of exactly that name; an alias
        // would only be a second name for it.
        let same_name = match resolved {
            Ty::Record(id) => self.types().record(id).rust_name == name.name,
            Ty::Enum(id) => self.types().enum_def(id).rust_name == name.name,
            _ => false,
        };
        if same_name {
            return;
        }
        let rust_name = self.reserve_item_name(&name.name);
        self.program.typedefs.push(TypedefItem {
            rust_name,
            ty: resolved,
            range: name.range,
        });
    }

    /// Gives an anonymous `struct { … }` or `enum { … }` the name of the
    /// `typedef` that introduces it, the way C programmers intend it to be
    /// read.
    ///
    /// Returns whether the tag took the name, in which case no alias item is
    /// needed.
    fn claim_anonymous_tag(&mut self, ty: Ty, name: &str) -> bool {
        let anonymous = match ty {
            Ty::Record(id) => self.types().record(id).anonymous,
            Ty::Enum(id) => self.types().enum_def(id).anonymous,
            _ => false,
        };
        if !anonymous || !self.try_reserve_item_name(name) {
            return false;
        }
        match ty {
            Ty::Record(id) => {
                let record = self.program.types.record_mut(id);
                record.rust_name = name.to_owned();
                record.anonymous = false;
            }
            Ty::Enum(id) => {
                let def = self.program.types.enum_mut(id);
                def.rust_name = name.to_owned();
                def.anonymous = false;
            }
            _ => unreachable!("only tags can be anonymous"),
        }
        true
    }

    // -- functions ----------------------------------------------------------

    /// Declares (but does not define) a function.
    ///
    /// `definition` carries the parameter declarations of a *definition*, which
    /// is what turns the check into "this is the definition".
    #[allow(clippy::too_many_arguments)]
    pub(super) fn declare_function(
        &mut self,
        decl: &ast::Decl,
        name: &ast::Ident,
        func: &ast::FunctionType,
        range: SourceRange,
        attrs: &ast::Attributes,
        asm_label: Option<&ast::Spanned<String>>,
        definition: Option<&ast::FunctionDef>,
    ) -> Option<FuncId> {
        let specifiers = &decl.specifiers;
        let is_static =
            specifiers.storage.as_ref().map(|s| s.node) == Some(ast::StorageClass::Static);
        let is_inline = specifiers.inline;
        let is_noreturn = specifiers.noreturn.is_some() || attrs.noreturn.is_some();
        let inline_hint = match (attrs.always_inline.is_some(), attrs.noinline.is_some()) {
            (true, false) => Some(ir::InlineHint::Always),
            (false, true) => Some(ir::InlineHint::Never),
            _ => None,
        };
        let init_kind = match (attrs.constructor.is_some(), attrs.destructor.is_some()) {
            (true, _) => Some(ir::InitKind::Constructor),
            (false, true) => Some(ir::InitKind::Destructor),
            _ => None,
        };
        if let Some(alignment) = &attrs.aligned {
            self.error(
                alignment.range,
                "'aligned' is not supported on a function; Rust has no way to say it",
            );
        }
        if let Some(packed) = attrs.packed {
            self.error(
                packed,
                "'packed' is only meaningful on a record or a member",
            );
        }
        // C23 has no `constexpr` functions, and neither has this: a constant
        // here is a value, folded wherever its name is used.
        if let Some(storage) = &specifiers.storage
            && storage.node == ast::StorageClass::Constexpr
        {
            let at = storage.range;
            self.error(at, "'constexpr' is not supported on a function");
        }

        if !func.kr_names.is_empty() || definition.is_some_and(|def| !def.kr_decls.is_empty()) {
            self.error(
                range,
                "old-style (K&R) function definitions are not supported; \
                 write a prototype instead",
            );
            return None;
        }
        // Declaring and *calling* a variadic function is ordinary stable Rust;
        // only defining one needs `c_variadic`, which Rust stabilised in 1.99.
        if func.variadic && definition.is_some() && !self.c_variadic {
            let at = func.ellipsis.unwrap_or(range);
            self.gate(
                at,
                "variadic function definitions require Rust 1.99 or later \
                 (this toolchain is older)",
            );
        }

        let ret = self.ty_of(&func.ret)?;
        if self.reject_va_list(ret, func.ret.range) {
            return None;
        }
        if !ret.is_void() && !self.types().is_complete(ret) {
            self.error(
                func.ret.range,
                format!(
                    "function cannot return an incomplete type '{}'",
                    self.tyname(ret)
                ),
            );
            return None;
        }

        let mut param_tys = Vec::with_capacity(func.params.len());
        let mut param_names = Vec::with_capacity(func.params.len());
        for param in &func.params {
            // A parameter is an object of automatic storage duration; the two
            // storage classes below cannot apply to one.
            if let Some(storage) = &param.specifiers.storage
                && matches!(
                    storage.node,
                    ast::StorageClass::Constexpr | ast::StorageClass::ThreadLocal
                )
            {
                self.error(
                    storage.range,
                    format!("'{}' is not allowed on a parameter", storage.node.as_str()),
                );
            }
            let ty = match self.resolve_param_ty(&param.ty) {
                Ok(ty) => ty,
                Err(err) => {
                    if !err.message.is_empty() {
                        self.error(err.range, err.message);
                    }
                    return None;
                }
            };
            if ty.is_void() {
                self.error(param.range, "parameter has incomplete type 'void'");
                return None;
            }
            if !self.types().is_complete(ty) {
                self.error(
                    param.range,
                    format!("parameter has incomplete type '{}'", self.tyname(ty)),
                );
                return None;
            }
            if definition.is_some() && param.name.is_none() {
                self.error(param.range, "parameter name omitted");
                return None;
            }
            param_tys.push(ty);
            param_names.push(param.name.as_ref().map(|n| n.name.clone()));
        }

        let sig = Signature {
            ret,
            params: param_tys,
            variadic: func.variadic,
            prototyped: self.is_prototyped(func),
        };

        let existing = match self.lookup(&name.name) {
            Some(Entry::Function(id)) => Some(*id),
            Some(other) => {
                let what = other.describe();
                let previous = match other {
                    Entry::Object(id) => self.program.object(*id).range,
                    Entry::Typedef(entry) => entry.range,
                    Entry::Constant { range, .. } => *range,
                    Entry::Function(_) => unreachable!("matched above"),
                };
                self.error_note(
                    name.range,
                    format!("redefinition of '{}', which is already {what}", name.name),
                    previous,
                    format!("previous declaration of '{}' is", name.name),
                );
                return None;
            }
            None => None,
        };

        let id = match existing {
            Some(id) => {
                let previous = self.program.function(id).clone();
                let Some(composite) = self.composite_signature(&previous.sig, &sig) else {
                    self.error_note(
                        name.range,
                        format!("conflicting types for '{}'", name.name),
                        previous.range,
                        format!("previous declaration of '{}' is", name.name),
                    );
                    return None;
                };
                // The composite type is what later calls are checked against;
                // a *definition* keeps its own signature instead, because that
                // is what the generated Rust item really takes. The two differ
                // only for `int f(int); int f() { … }`, which C allows and
                // which defines a function that ignores its argument.
                let merged = if definition.is_some() {
                    sig.clone()
                } else if previous.body.is_some() {
                    previous.sig.clone()
                } else {
                    composite
                };
                // The parameter names the `extern` block declares come from
                // whichever declaration supplied the signature.
                let names_from_here = definition.is_some() || merged == sig;
                if definition.is_some() && previous.body.is_some() {
                    self.error_note(
                        name.range,
                        format!("redefinition of '{}'", name.name),
                        previous.range,
                        format!("previous definition of '{}' is", name.name),
                    );
                    return None;
                }
                let entry = &mut self.program.functions[id.0 as usize];
                entry.sig = merged;
                entry.is_static |= is_static;
                entry.is_inline |= is_inline;
                entry.noreturn |= is_noreturn;
                entry.cold |= attrs.cold.is_some();
                entry.inline_hint = entry.inline_hint.or(inline_hint);
                entry.init_kind = entry.init_kind.or(init_kind);
                entry.deprecated = entry
                    .deprecated
                    .take()
                    .or_else(|| attrs.deprecated.as_ref().map(|d| d.node.clone()));
                entry.section = entry
                    .section
                    .take()
                    .or_else(|| attrs.section.as_ref().map(|s| s.node.clone()));
                entry.asm_label = entry
                    .asm_label
                    .take()
                    .or_else(|| asm_label.map(|label| label.node.clone()));
                if definition.is_some() {
                    entry.range = name.range;
                }
                if names_from_here {
                    entry.param_names = param_names;
                }
                id
            }
            None => {
                let id = FuncId(self.program.functions.len() as u32);
                self.program.functions.push(Function {
                    name: name.name.clone(),
                    sig,
                    params: Vec::new(),
                    param_names,
                    is_static,
                    is_inline,
                    noreturn: is_noreturn,
                    inline_hint,
                    cold: attrs.cold.is_some(),
                    deprecated: attrs.deprecated.as_ref().map(|d| d.node.clone()),
                    section: attrs.section.as_ref().map(|s| s.node.clone()),
                    asm_label: asm_label.map(|label| label.node.clone()),
                    init_kind,
                    locals: Vec::new(),
                    body: None,
                    range: name.range,
                });
                self.item_names.insert(name.name.clone());
                self.insert_at_file_scope(&name.name, Entry::Function(id));
                id
            }
        };
        Some(id)
    }

    pub(super) fn function_def(&mut self, def: &ast::FunctionDef) {
        let ast::TypeKind::Function(func) = &def.ty.kind else {
            return;
        };
        let decl = ast::Decl {
            specifiers: def.specifiers.clone(),
            declarators: Vec::new(),
            range: def.range,
        };
        let mut attrs = def.attrs.clone();
        attrs.merge(def.specifiers.attrs.clone());
        let Some(id) = self.declare_function(
            &decl,
            &def.name,
            func,
            def.ty.range,
            &attrs,
            def.asm_label.as_ref(),
            Some(def),
        ) else {
            return;
        };

        // Parameters live in the same scope as the body's outermost block, so
        // that `int f(int x) { int x; }` is the redefinition C says it is.
        self.push_scope();
        self.ret_ty = self.program.function(id).sig.ret;
        self.func_name = def.name.name.clone();
        self.func_variadic = func.variadic;
        self.va_param = None;
        self.breakables.clear();
        self.switch_stack.clear();
        self.next_loop = 0;
        self.next_switch = 0;
        self.next_label = 0;
        // Whether the body can keep Rust's own control flow is decided before
        // it is checked, because it changes how `switch` is lowered.
        self.cfg_mode = Self::needs_cfg(&def.body);
        self.collect_labels(&def.body);

        let mut params = Vec::with_capacity(func.params.len());
        for (param, ty) in func
            .params
            .iter()
            .zip(self.program.function(id).sig.params.clone())
        {
            let Some(name) = &param.name else { continue };
            if self.check_redefinition(name) {
                // Two parameters under one name; leaving the second out keeps
                // the generated signature from being invalid Rust on top of
                // being invalid C.
                continue;
            }
            let object = self.new_object(
                &name.name,
                ty,
                Storage::Automatic,
                param.ty.qualifiers.is_const,
                name.range,
            );
            self.insert(&name.name, Entry::Object(object));
            if ty.is_va_list() {
                // A parameter of a *definition*: the generated signature has
                // to name `core::ffi::VaList`.
                self.gate_va_list(name.range);
                if self.va_param.is_none() {
                    self.va_param = Some(object);
                }
            }
            params.push(object);
        }
        self.func_params = params.clone();

        // Every automatic object the body declares — including the ones
        // buried in a statement expression, which no walk over the statements
        // would find — is the function's, so the range of ids the body used is
        // what code generation names its locals from.
        let first_object = self.program.objects.len() as u32;
        let mut body = self.block_items(&def.body.items);
        let ret = self.ret_ty;
        // A C function may fall off its end; the value is then whatever the ABI
        // left behind. Returning a zero is the honest, safe translation —
        // `unreachable_unchecked` would turn a legal (if useless) C program
        // into undefined behaviour. In CFG mode the `return` is unconditional:
        // every block needs a terminator, and an unreachable one is dropped.
        // A call to a `_Noreturn` function ends the statement it is in, which
        // is why the function table is needed here.
        let terminates = ir::always_terminates(&body, &self.program.functions);
        if self.cfg_mode || (!ret.is_void() && !terminates) {
            let value = (!ret.is_void()).then(|| self.zero(ret, def.body.range));
            body.push(Stmt::Return {
                value,
                range: def.body.range,
            });
        }
        self.pop_scope();

        let body = if self.cfg_mode {
            ir::Body::Cfg(crate::cfg::lower(body, &params, &self.program.objects))
        } else {
            ir::Body::Structured(body)
        };
        let last_object = self.program.objects.len() as u32;
        let locals: Vec<ObjectId> = (first_object..last_object)
            .map(ObjectId)
            .filter(|id| self.program.object(*id).storage == Storage::Automatic)
            .collect();
        let entry = &mut self.program.functions[id.0 as usize];
        entry.params = params;
        entry.locals = locals;
        entry.body = Some(body);
    }

    // -- static initialisers ------------------------------------------------

    /// Reduces an initialiser for an object with static storage duration to
    /// something the generated `static mut` item can hold.
    pub(super) fn static_init(&mut self, expr: Expr, what: &str) -> Option<Expr> {
        let (ty, range) = (expr.ty, expr.range);
        match expr.kind {
            ExprKind::RecordLit { record, fields } => {
                let fields: Option<Vec<Expr>> = fields
                    .into_iter()
                    .map(|f| self.static_init(f, what))
                    .collect();
                Some(Expr::new(
                    ExprKind::RecordLit {
                        record,
                        fields: fields?,
                    },
                    ty,
                    range,
                ))
            }
            ExprKind::UnionLit {
                record,
                index,
                value,
            } => {
                let value = self.static_init(*value, what)?;
                Some(Expr::new(
                    ExprKind::UnionLit {
                        record,
                        index,
                        value: Box::new(value),
                    },
                    ty,
                    range,
                ))
            }
            ExprKind::ArrayLit(items) => {
                let items: Option<Vec<Expr>> = items
                    .into_iter()
                    .map(|e| self.static_init(e, what))
                    .collect();
                Some(Expr::new(ExprKind::ArrayLit(items?), ty, range))
            }
            ExprKind::ArrayRepeat { value, len } => {
                let value = self.static_init(*value, what)?;
                Some(Expr::new(
                    ExprKind::ArrayRepeat {
                        value: Box::new(value),
                        len,
                    },
                    ty,
                    range,
                ))
            }
            ExprKind::Zeroed => Some(Expr::new(ExprKind::Zeroed, ty, range)),
            // A compound literal at file scope is an object of its own, and
            // reading one where a constant expression has to go is reading a
            // value that was constant when it was written. ISO C does not
            // allow it; GCC does, and c-testsuite `00216` writes it.
            ExprKind::Load(Place {
                kind: PlaceKind::Object(id),
                ..
            }) if self.static_literals.contains_key(&id) => {
                let at = self.static_literals[&id];
                let value = self.program.statics[at].init.clone();
                self.static_init(value, what)
            }
            _ if ty.is_arithmetic() => {
                let value = self.const_eval_at(&expr, what)?;
                Some(self.const_to_expr(value, ty, range))
            }
            _ if ty.is_pointer() => {
                if self.is_address_constant(&expr) {
                    return Some(expr);
                }
                self.error(
                    range,
                    format!("{what} is not a compile-time constant expression"),
                );
                None
            }
            _ => {
                self.error(
                    range,
                    format!("{what} is not a compile-time constant expression"),
                );
                None
            }
        }
    }

    /// Whether a pointer value is one the linker can work out: a null pointer,
    /// the address of something with static storage duration, or a constant
    /// offset from one.
    fn is_address_constant(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Zeroed | ExprKind::FuncAddr(_) => true,
            ExprKind::Int(v) => *v == 0,
            ExprKind::Cast(inner) => self.is_address_constant(inner),
            ExprKind::AddrOf(place) => self.is_static_place(place),
            ExprKind::PtrOffset { ptr, index, .. } => {
                self.is_address_constant(ptr) && matches!(index.kind, ExprKind::Int(_))
            }
            _ => false,
        }
    }

    fn is_static_place(&self, place: &Place) -> bool {
        match &place.kind {
            PlaceKind::Object(id) => {
                !matches!(self.program.object(*id).storage, Storage::Automatic)
            }
            PlaceKind::Str(_) => true,
            PlaceKind::Field { base, .. } => self.is_static_place(base),
            PlaceKind::Index { base, index } => {
                self.is_address_constant(base) && matches!(index.kind, ExprKind::Int(_))
            }
            PlaceKind::Deref(ptr) => self.is_address_constant(ptr),
            // A block-scope compound literal has automatic storage duration,
            // so its address is not something the linker can work out; one at
            // file scope is an ordinary `Object` with static storage.
            PlaceKind::Temporary(_) | PlaceKind::CompoundLiteral { .. } => false,
        }
    }
}
