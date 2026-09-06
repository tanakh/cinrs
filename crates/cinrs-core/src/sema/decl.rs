//! Declarations: objects, `typedef`s, functions and the `extern` block.

use std::collections::HashSet;

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, Expr, ExprKind, FuncId, Function, ObjectId, Place, PlaceKind, Signature, StaticVar, Stmt,
    Storage, Ty, TypedefItem,
};

use super::types::Completeness;
use super::{ConvContext, Entry, FuncScope, NestFrame, SavedFunc, Sema, TypedefEntry};

/// What a declaration's `_Thread_local` specifier came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ThreadLocal {
    /// There was none.
    No,
    /// The object is thread-local, and the declaration is well formed.
    Yes,
    /// The declaration was reported; nothing more should be made of it.
    Rejected,
}

impl Sema<'_> {
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
            self.declare_sole_tag(decl);
            self.standalone_declaration(decl, |sema| {
                let _ = sema.ty_of(&decl.specifiers.base);
            });
            return;
        }
        for declarator in &decl.declarators {
            self.declarator(decl, declarator, true);
        }
    }

    /// C11 6.7.2.3p7: `struct S;` **on its own** declares the tag in the scope
    /// it is written in, and therefore a *new* type, even where an enclosing
    /// scope has a `struct S` of its own.
    ///
    /// Only the sole declaration does that. `struct S *p;` is 6.7.2.3p8 —
    /// "and no other declaration of the identifier as a tag is visible" — and
    /// names the visible tag. WG14 DR088 is the difference: a `struct S;`
    /// written inside a block makes the file scope's `struct S *` and this
    /// block's two incompatible pointer types, and `drs/dr0xx.c` checks both
    /// halves of it.
    ///
    /// The tag goes in before the specifier is resolved, which is what makes
    /// [`Sema::record_ty`] find it in *this* scope and stop looking outward.
    /// A definition (`struct S { … };`) already declares a new type and needs
    /// nothing here, and neither does `enum`, which has no such form —
    /// footnote 130's "a similar construction with `enum` does not exist".
    /// Marks a declaration that declares nothing but its specifier, which is
    /// the one place C23 lets a non-defining `enum E : T` stand.
    ///
    /// See [`Sema::standalone_enum`].
    pub(super) fn standalone_declaration(&mut self, decl: &ast::Decl, f: impl FnOnce(&mut Self)) {
        let outer = match &decl.specifiers.base.kind {
            ast::TypeKind::Enum(id) => self.standalone_enum.replace(*id),
            _ => self.standalone_enum.take(),
        };
        f(self);
        self.standalone_enum = outer;
    }

    pub(super) fn declare_sole_tag(&mut self, decl: &ast::Decl) {
        let ast::TypeKind::Record(spec_id) = &decl.specifiers.base.kind else {
            return;
        };
        if self.record_by_spec[spec_id.index()].is_some() {
            return;
        }
        let spec = self.record_spec(*spec_id);
        if spec.fields.is_some() {
            return;
        }
        let Some(name) = spec.name.as_ref().map(|n| n.name.clone()) else {
            return;
        };
        // A tag this scope already has — as a record or as an enumeration — is
        // this declaration's subject, or its diagnostic; either way there is
        // nothing new to declare.
        if self.tag_here(&name).is_some() {
            return;
        }
        let kind = match spec.kind {
            ast::RecordKind::Struct => crate::ir::RecordKind::Struct,
            ast::RecordKind::Union => crate::ir::RecordKind::Union,
        };
        let range = spec.range;
        let id = self.declare_record(kind, Some(name.clone()), range);
        self.insert_tag(&name, super::TagEntry::Record(id));
        self.record_by_spec[spec_id.index()] = Some(id);
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
            let mut attrs = declarator.attrs.clone();
            attrs.merge(decl.specifiers.attrs.clone());
            self.reject_cleanup(&attrs, "a 'typedef'");
            return self.declare_typedef(name, &declarator.ty, declarator.init.as_ref(), &attrs);
        }

        let storage = decl.specifiers.storage.as_ref().map(|s| s.node);
        if let ast::TypeKind::Function(func) = &declarator.ty.kind {
            // C11 6.7.1p4: `_Thread_local` applies to an object, and a
            // function is not one.
            if let Some(range) = decl.specifiers.thread_local {
                self.error(range, "'_Thread_local' is not allowed on a function");
            }
            let mut attrs = declarator.attrs.clone();
            attrs.merge(decl.specifiers.attrs.clone());
            // GNU spells the forward declaration of a nested function
            // `auto int g(int);`, which is the one way to write two nested
            // functions that call each other. At file scope the storage class
            // means nothing a function can have.
            let scope = match storage {
                Some(ast::StorageClass::Auto) if !file_scope => FuncScope::Nested,
                Some(ast::StorageClass::Auto) => {
                    let at = decl
                        .specifiers
                        .storage
                        .as_ref()
                        .map_or(name.range, |s| s.range);
                    self.error(
                        at,
                        "'auto' is not allowed on a file-scope function; it declares a nested \
                         function, which only a block may hold",
                    );
                    FuncScope::File
                }
                _ => FuncScope::File,
            };
            self.declare_function(
                decl,
                name,
                func,
                declarator.ty.range,
                &attrs,
                declarator.asm_label.as_ref(),
                None,
                scope,
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
        // GCC drops `cleanup` on anything but an automatic object, with
        // "'cleanup' attribute ignored"; dropping it silently would change
        // what the program does, so it is refused with the reason.
        if let Some(what) = match storage {
            _ if decl.specifiers.thread_local.is_some() => Some("a thread-local object"),
            Some(ast::StorageClass::Static) => Some("an object with static storage duration"),
            Some(ast::StorageClass::Extern) => Some("an 'extern' declaration"),
            Some(ast::StorageClass::Constexpr) => Some("a 'constexpr' object"),
            _ if file_scope => Some("an object at file scope"),
            _ => None,
        } {
            self.reject_cleanup(&attrs, what);
        }

        let thread_local = self.check_thread_local(decl, storage, file_scope);
        if thread_local == ThreadLocal::Rejected {
            // The name is still declared, as something already reported, so
            // that every use of it adds nothing to the diagnostics.
            self.check_redefinition(name);
            let id = self.new_object(&name.name, Ty::Error, Storage::Automatic, false, name.range);
            self.insert(&name.name, Entry::Object(id));
            return Vec::new();
        }
        if storage == Some(ast::StorageClass::Extern) {
            self.declare_extern_object(name, declarator);
            if let Some(Entry::Object(id)) = self.lookup(&name.name).cloned() {
                self.apply_object_attributes(id, &attrs, declarator);
            }
            return Vec::new();
        }

        // C99 6.2.1p7: "the scope of an identifier … begins just after the
        // completion of its declarator", which puts the object in scope for
        // its *own* initialiser. `struct list head = { &head, &head }` is the
        // circular-list idiom, `T *p = malloc(sizeof *p)` is the allocation
        // one, and `struct E e[2] = { { 0, &e[1] }, … }` is a torture case.
        // So the type is resolved and the object declared *before* the
        // initialiser is checked.
        //
        // The two forms whose type the initialiser decides — C23's `auto x =
        // e` and `T x[] = { … }` — cannot do that, because there is no type to
        // declare the object with yet. Neither of them can name itself either
        // (an incomplete array has no `sizeof`), so nothing is lost.
        //
        // Every bound that is not a constant expression gets a hidden object
        // of its own during the resolution below, and the statements that bind
        // them are this declaration's to take.
        self.vm_bounds.clear();
        let mut inferred = type_from_initializer(declarator);
        let (mut ty, mut init) = if inferred {
            self.typed_initializer(declarator, &name.name)
                .unwrap_or((Ty::Error, None))
        } else {
            // A file-scope declaration with no initialiser is a tentative
            // definition, and one of the two places an incomplete array type
            // may be the type of an object (6.9.2p3); `extern` is the other,
            // and went through `declare_extern_object` above.
            let completeness = if file_scope {
                Completeness::TentativeArray
            } else {
                Completeness::Required
            };
            let ty = self
                .declared_object_ty_of(&declarator.ty, &name.name, completeness)
                .unwrap_or(Ty::Error);
            // A tentative definition of an enumeration that has no list yet:
            // the tag may still be completed later in the unit, so the answer
            // waits for `Sema::check_tentative_enums`.
            if file_scope && let Some(tag) = self.incomplete_enum(&declarator.ty).map(str::to_owned)
            {
                self.incomplete_enum_objects
                    .push((name.name.clone(), tag, declarator.range));
            }
            (self.apply_mode(ty, &attrs), None)
        };
        // `typedef int A[]; A a = { 1, 2 };` — an incomplete array type reached
        // through a `typedef` takes its length from the initialiser too
        // (6.7.9p22), exactly as the `int a[] = { … }` spelling does. It is the
        // only shape of that rule the declarator itself does not show.
        if !inferred
            && let Some(list) = &declarator.init
            && let Ty::Array(id) = ty
            && self.types().array_type(id).incomplete
        {
            let array = self.types().array_type(id);
            if let Some((completed, value)) =
                self.init_array_inferred(list, array.elem, array.elem_const, &name.name)
            {
                ty = completed;
                init = Some(value);
                inferred = true;
            }
        }
        let bounds = self.take_vm_bounds();

        if storage == Some(ast::StorageClass::Constexpr) {
            // A `constexpr` object is a *constant* rather than storage, so
            // there is no address for its own initialiser to take and nothing
            // to declare before checking one.
            if !inferred {
                init = self.late_initializer(declarator, ty, &name.name);
            }
            self.declare_constexpr(name, ty, init, declarator);
            return Vec::new();
        }
        let is_const = declarator.ty.qualifiers.is_const;
        let is_static = storage == Some(ast::StorageClass::Static);

        // A variably modified type is the one type whose object cannot be a
        // plain `let`, and the one C hedges around with rules about where it
        // may be declared at all. It may not have an initialiser at all.
        if self.types().is_vm(ty) {
            let mut out = self.declare_vla(name, decl, declarator, ty, bounds, file_scope);
            if let Some(Stmt::Vla(def)) = out.last() {
                let object = def.object;
                out.extend(self.declare_cleanup(object, attrs.cleanup.as_ref()));
            }
            return out;
        }

        if file_scope || is_static {
            // C99 6.7.5.2p2: an object with static storage duration may not
            // have a variably modified type — `static int (*p)[n];` has no
            // moment at which its bound could be evaluated. Having evaluated
            // one is exactly what says the type is.
            if !bounds.is_empty() {
                self.error(
                    declarator.range,
                    "a variably modified type cannot have static storage duration",
                );
            }
            // An object with static storage duration outlives every argument
            // list there could be, and Rust's `VaList` says so with a lifetime
            // no item could name.
            let ty = if self.reject_va_list(ty, declarator.range) {
                Ty::Error
            } else {
                ty
            };
            let object = self.declare_static_object(
                name,
                ty,
                is_const,
                is_static,
                file_scope,
                thread_local == ThreadLocal::Yes,
                declarator,
            );
            if !inferred {
                init = self.late_initializer(declarator, ty, &name.name);
            }
            if let (Some(id), Some(init)) = (object, init) {
                self.initialize_static_object(id, declarator, init);
            }
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
        // C11 6.7.1p6: the address of a `register` object cannot be computed,
        // explicitly or by an array decaying to a pointer. See
        // [`ir::Object::is_register`].
        self.program.objects[id.0 as usize].is_register =
            storage == Some(ast::StorageClass::Register);
        self.insert(&name.name, Entry::Object(id));
        if ty.is_error() {
            return Vec::new();
        }
        if !inferred {
            init = self.late_initializer(declarator, ty, &name.name);
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
        // Rust cannot name a binding in its own initialiser, so an automatic
        // object whose initialiser names *itself* is defined with the zero
        // C would have left there and assigned afterwards. The assignment
        // stands where the declaration was written, which is where C evaluates
        // the initialiser; a hoisted definition keeps the zero, which is what
        // `explicit: false` asks for.
        // A bound written in the declarator — `int (*p)[n]` — is evaluated
        // where the declaration stands, into the hidden object the type points
        // at, and that has to happen before anything can use the type.
        let mut out = bounds;
        if explicit && ir::mentions_object(&init, id) {
            let zero = self.zero(ty, declarator.range);
            let place = super::place_of(PlaceKind::Object(id), ty, false, name.range);
            let assign = Expr::new(
                ExprKind::Assign {
                    place,
                    value: Box::new(init),
                },
                ty,
                declarator.range,
            );
            out.push(Stmt::Let {
                object: id,
                init: zero,
                explicit: false,
            });
            out.push(Stmt::Expr(assign));
        } else {
            out.push(Stmt::Let {
                object: id,
                init,
                explicit,
            });
        }
        out.extend(self.declare_cleanup(id, attrs.cleanup.as_ref()));
        out
    }

    /// Registers `T x __attribute__((cleanup(f)));` on an object that has just
    /// been declared (GCC's extension).
    ///
    /// The registration is a statement rather than a property of the object
    /// because *where* it stands is what it means: the guard the structured
    /// lowering binds goes right after the object's own binding, so that
    /// Rust's drop order is C's reverse declaration order, and the
    /// [CFG](crate::cfg) lowering reads the same statement as "from here to
    /// the end of the block, this call is owed on every way out".
    pub(super) fn declare_cleanup(
        &mut self,
        object: ObjectId,
        cleanup: Option<&ast::Cleanup>,
    ) -> Option<Stmt> {
        let cleanup = cleanup?;
        // GCC's own two words for the argument it cannot use.
        let Some(name) = &cleanup.func else {
            self.error(cleanup.range, "cleanup argument not an identifier");
            return None;
        };
        let Some(Entry::Function(func)) = self.lookup(&name.name).cloned() else {
            self.error(name.range, "cleanup argument not a function");
            return None;
        };
        // The structured lowering holds the cleanup function in a drop guard,
        // which is its *address*; a lifted nested function that uses the
        // enclosing frame has none to give.
        if self.program.function(func).is_nested() {
            self.nested_addresses.push((func, name.range));
        }
        let entry = self.program.function(func);
        let (sig, declared, fname) = (entry.sig.clone(), entry.range, entry.name.clone());
        if sig.params.len() != 1 || sig.variadic {
            let expected = if sig.variadic {
                format!("at least {}", sig.params.len())
            } else {
                sig.params.len().to_string()
            };
            let word = if sig.params.len() > 1 { "few" } else { "many" };
            self.error_note(
                cleanup.range,
                format!(
                    "the cleanup function is called with one argument, the object's address: \
                     too {word} arguments to function call, expected {expected}, have 1"
                ),
                declared,
                format!("'{fname}' is declared"),
            );
            return None;
        }
        let param = sig.params[0];
        let info = self.program.object(object);
        let (ty, is_const, range) = (info.ty, info.is_const, info.range);
        if ty.is_error() {
            return None;
        }
        let address = Expr::new(
            ExprKind::AddrOf(super::place_of(
                PlaceKind::Object(object),
                ty,
                is_const,
                range,
            )),
            self.ptr_to(ty, is_const),
            range,
        );
        let arg = self.convert_for(
            address,
            param,
            super::ConvContext::Argument {
                index: 1,
                func: fname,
            },
        );
        let call = Expr::new(
            ExprKind::Call {
                callee: ir::Callee::Direct(func),
                args: vec![arg],
            },
            sig.ret,
            range,
        );
        self.cleanup_depth += 1;
        Some(Stmt::Cleanup(Box::new(ir::CleanupDef {
            object,
            func,
            param,
            call,
            range: cleanup.range,
        })))
    }

    /// Reports a `cleanup` attribute somewhere it cannot mean anything.
    ///
    /// GCC drops it with "'cleanup' attribute ignored"; ignoring it here would
    /// change what the program does, so it is refused with the reason instead.
    pub(super) fn reject_cleanup(&mut self, attrs: &ast::Attributes, what: &str) {
        if let Some(cleanup) = &attrs.cleanup {
            self.error(
                cleanup.range,
                format!(
                    "'cleanup' attribute ignored on {what}: it calls the function when the \
                     object goes out of scope, and only an object with automatic storage \
                     duration ever does"
                ),
            );
        }
    }

    /// Checks a `_Thread_local` object declaration (C11 6.7.1).
    ///
    /// The three spellings — `_Thread_local`, C23's `thread_local` and GNU's
    /// `__thread` — mean the same thing, and the parser has already gated the
    /// two that a revision introduced. What is left is where the specifier may
    /// appear and what this crate can generate for it:
    ///
    /// * at block scope it needs `static` or `extern` (6.7.1p3), because an
    ///   object with automatic storage duration is per *call*, not per thread;
    /// * `extern` is refused: naming a TLS symbol another object file defines
    ///   needs Rust's `#[thread_local]` on an `extern` item, which is unstable;
    /// * a function is not an object (6.7.1p4);
    /// * and the object's initialiser must be a constant expression, which the
    ///   static-initialiser path enforces on its own.
    ///
    /// [`ThreadLocal::Rejected`] means the declaration was reported and the
    /// caller should register the name as an error and stop.
    fn check_thread_local(
        &mut self,
        decl: &ast::Decl,
        storage: Option<ast::StorageClass>,
        file_scope: bool,
    ) -> ThreadLocal {
        let Some(range) = decl.specifiers.thread_local else {
            return ThreadLocal::No;
        };
        if storage == Some(ast::StorageClass::Extern) {
            self.error(
                range,
                "an 'extern' thread-local object is not supported: reaching a TLS symbol \
                 defined elsewhere needs Rust's `#[thread_local]`, which is unstable. \
                 Define the object in this unit instead",
            );
            return ThreadLocal::Rejected;
        }
        if matches!(
            storage,
            Some(ast::StorageClass::Auto | ast::StorageClass::Register)
        ) {
            self.error(
                range,
                "'_Thread_local' cannot be combined with 'auto' or 'register'; it goes with \
                 'static' or 'extern', or on its own at file scope",
            );
            return ThreadLocal::Rejected;
        }
        if storage == Some(ast::StorageClass::Constexpr) {
            self.error(
                range,
                "'_Thread_local' cannot be combined with 'constexpr'; a constant has no \
                 storage to give a thread a copy of",
            );
            return ThreadLocal::Rejected;
        }
        if !file_scope && storage != Some(ast::StorageClass::Static) {
            // C11 6.7.1p3. GCC says the same thing in the same place.
            self.error(
                range,
                "'_Thread_local' on a block-scope object needs 'static' or 'extern': the \
                 object has static storage duration, one copy per thread",
            );
            return ThreadLocal::Rejected;
        }
        // A variably modified type needs nothing here: the object has static
        // storage duration either way, and the variable-length-array path
        // already says so in more useful words.
        ThreadLocal::Yes
    }

    /// Declares an object of variably modified type: `T a[n];`, `T a[n][m];`
    /// (C99 6.7.5.2).
    ///
    /// The elements live in one hidden `Vec` — however many dimensions there
    /// are — and the object itself is a pointer into it; see [`ir::VlaDef`].
    /// `bounds` are the statements that bind the hidden length objects the
    /// type carries, which have to run before the allocation does. Everything
    /// C forbids about such a declaration is reported here, because a
    /// block-scope object with automatic storage duration is the only place a
    /// variably modified *object* is allowed to reach at all.
    fn declare_vla(
        &mut self,
        name: &ast::Ident,
        decl: &ast::Decl,
        declarator: &ast::InitDeclarator,
        ty: Ty,
        bounds: Vec<Stmt>,
        file_scope: bool,
    ) -> Vec<Stmt> {
        // C99 6.7.5.2 introduced them (N683), so a `c89!` block is told to
        // write a different macro rather than being told the bound is not a
        // constant expression.
        self.require_standard(
            crate::Standard::C99,
            "a variable length array",
            declarator.range,
        );
        let storage = decl.specifiers.storage.as_ref().map(|s| s.node);
        let unknown = self.types().vm_dims(ty).contains(&ir::VmDim::Unknown);
        let problem = if file_scope || storage == Some(ast::StorageClass::Static) {
            // An object with static storage duration is an item whose size the
            // linker has to know, and there is no moment at which the bound
            // could be evaluated.
            Some("a variable length array cannot have static storage duration".to_owned())
        } else if declarator.init.is_some() {
            // C99 6.7.8p3: there would be nothing to check the number of
            // initialisers against.
            Some("a variable length array cannot have an initializer".to_owned())
        } else if unknown {
            // The type arrived from somewhere that carries no bound with it:
            // `int a[*]`, or `typeof` of a parameter declared in a prototype.
            Some(
                "the length of this variably modified type is not available here: \
                 its bound was never evaluated"
                    .to_owned(),
            )
        } else {
            None
        };
        if let Some(message) = problem {
            self.error(declarator.range, message);
            // The name is still declared, as something already reported, so
            // that using it adds nothing to the diagnostics.
            self.check_redefinition(name);
            let id = self.new_object(&name.name, Ty::Error, Storage::Automatic, false, name.range);
            self.insert(&name.name, Entry::Object(id));
            return Vec::new();
        }

        let is_const = declarator.ty.qualifiers.is_const;
        self.check_redefinition(name);
        let object = self.new_object(&name.name, ty, Storage::Automatic, is_const, name.range);
        self.insert(&name.name, Entry::Object(object));
        // The `Vec` is not an object of the C program; its type is what is
        // left under the variable dimensions, and code generation knows to
        // spell the binding `Vec<T>`.
        let elem = self.types().vm_step_ty(ty);
        let storage = self.new_object(
            &format!("__cinrs_vla_{}", name.name),
            elem,
            Storage::Automatic,
            false,
            name.range,
        );
        self.program.objects[storage.0 as usize].vla_storage = true;
        let count = self
            .vm_count(ty, Vec::new(), declarator.range, declarator.range)
            .expect("every dimension has a length object");
        let mut out = bounds;
        out.push(Stmt::Vla(Box::new(ir::VlaDef {
            object,
            storage,
            count,
            range: declarator.range,
        })));
        out
    }

    /// Puts what `__asm__("symbol")` and `__attribute__((section("…")))`
    /// asked for onto an object with static storage duration.
    fn apply_object_attributes(
        &mut self,
        id: ObjectId,
        attrs: &ast::Attributes,
        declarator: &ast::InitDeclarator,
    ) {
        // A thread-local object is a `thread_local!` item rather than a
        // `static`, and neither of these has anywhere to go on one: a
        // `link_section` and a symbol name both describe a linker symbol that
        // a `thread_local!` does not have.
        if self.program.object(id).storage.is_thread_local() {
            for range in [
                declarator.asm_label.as_ref().map(|label| label.range),
                attrs.section.as_ref().map(|section| section.range),
            ]
            .into_iter()
            .flatten()
            {
                self.error(
                    range,
                    "a '_Thread_local' object has no linker symbol to name or to place in a \
                     section: it becomes a `thread_local!` item",
                );
            }
            return;
        }
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

    /// Checks the initialiser of a declarator whose type came from the
    /// declarator alone, after the object has been declared.
    ///
    /// See [`Sema::declarator`] for why the two are in that order. The
    /// [`type_from_initializer`] forms have already had theirs checked by
    /// [`Sema::typed_initializer`] and never reach here.
    fn late_initializer(
        &mut self,
        declarator: &ast::InitDeclarator,
        ty: Ty,
        name: &str,
    ) -> Option<Expr> {
        let init = declarator.init.as_ref()?;
        // A variable length array may not have one at all (C99 6.7.8p3);
        // `Sema::declare_vla` says so, and checking a list against a length
        // nobody knows would only add noise on top. A type that did not check
        // out has already been reported.
        if ty.is_error() || self.types().is_vm(ty) {
            return None;
        }
        self.initializer(init, ty, name)
    }

    /// Resolves a declarator's type and its initialiser together.
    ///
    /// They cannot be separated: `char s[] = "hi"` takes the array's length
    /// from the initialiser, and the initialiser needs the element type. Only
    /// the [`type_from_initializer`] forms come here; everything else resolves
    /// its type first, so that the object is in scope for its own initialiser.
    /// `auto *ptr = &a;` — Clang's extension, where the deduction happens
    /// through a run of pointer derivations.
    ///
    /// The type the declarator asks for is `T` under `depth` pointers, so the
    /// object's type is simply the initialiser's *own* type as long as it has
    /// that many levels to peel; what is deduced is what is left under them.
    /// See [`auto_pointer_depth`].
    fn auto_through_pointers(
        &mut self,
        declarator: &ast::InitDeclarator,
        name: &str,
        depth: usize,
    ) -> Option<(Ty, Option<Expr>)> {
        let init = declarator.init.as_ref()?;
        let ast::InitializerKind::Expr(expr) = &init.kind else {
            self.error(
                init.range,
                format!("the type of '{name}' cannot be inferred from a braced initializer"),
            );
            return None;
        };
        let stars = "*".repeat(depth);
        let value = self.underspecified(name, |sema| sema.expr(expr))?;
        let mut peeled = value.ty;
        for _ in 0..depth {
            match self.pointee(peeled) {
                Some(inner) => peeled = inner,
                None => {
                    self.error(
                        expr.range,
                        format!(
                            "'{name}', of type 'auto {stars}', has an incompatible initializer \
                             of type '{}'",
                            self.tyname(value.ty)
                        ),
                    );
                    return None;
                }
            }
        }
        if peeled.is_error() {
            return None;
        }
        Some((value.ty, Some(value)))
    }

    /// Checks something with the name of an **underspecified** declaration —
    /// C23's `auto x = e;` — recorded, so that naming it inside its own
    /// initialiser is refused rather than resolving to whatever an enclosing
    /// scope had.
    ///
    /// The identifier is in scope for its own initialiser (6.2.1p7), and for
    /// an inferred type there is nothing for it to name: `double b = 9; {
    /// auto b = b * b; }` is a constraint violation and not a use of the outer
    /// `b`, which is what `C23/n3007.c` checks and what GCC calls
    /// "underspecified 'b' referenced in its initializer".
    fn underspecified<T>(&mut self, name: &str, f: impl FnOnce(&mut Self) -> T) -> T {
        let outer = self.underspecified.replace(name.to_owned());
        let out = f(self);
        self.underspecified = outer;
        out
    }

    fn typed_initializer(
        &mut self,
        declarator: &ast::InitDeclarator,
        name: &str,
    ) -> Option<(Ty, Option<Expr>)> {
        if let Some(depth) = auto_pointer_depth(&declarator.ty) {
            return self.auto_through_pointers(declarator, name, depth);
        }
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
            let value = self.underspecified(name, |sema| sema.expr(expr))?;
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
            // `_Atomic auto x = 12;` is `_Atomic int`: N3007 keeps `_Atomic`
            // out of the "no other type specifier beside `auto`" rule
            // precisely because in this spelling it is a *qualifier*, and it
            // is the one qualifier an inferred type has to carry itself —
            // `const` and `volatile` live on the object. `_Atomic(auto)`, the
            // specifier spelling, is a different thing and is refused where
            // every other use of `auto` in a type name is.
            if declarator.ty.qualifiers.is_atomic {
                return match self.make_atomic(value.ty, declarator.ty.range) {
                    Ok(ty) => {
                        let value = self.convert_for(value, ty, ConvContext::Init(name.to_owned()));
                        Some((ty, Some(value)))
                    }
                    Err(err) => {
                        self.report_type_error(err);
                        None
                    }
                };
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
            if self.types().is_vm(element) {
                // `int x[][n] = { … }`: the number of elements would have to
                // come from the initialiser, and a variably modified object
                // may not have one at all (C99 6.7.8p3).
                self.error(
                    declarator.ty.range,
                    "a variable length array cannot have an initializer",
                );
                return None;
            }
            let elem_const = elem.qualifiers.is_const;
            // Without one the type is an *incomplete* array type, which is a
            // type rather than a mistake; `Sema::declarator` never sends such
            // a declarator here.
            let init = declarator
                .init
                .as_ref()
                .expect("`type_from_initializer` requires one");
            let (ty, expr) = self.init_array_inferred(init, element, elem_const, name)?;
            return Some((ty, Some(expr)));
        }

        unreachable!("only the `type_from_initializer` forms reach here")
    }

    /// Declares a file-scope object or a function-local `static`, *without*
    /// its initialiser.
    ///
    /// The object exists before the initialiser is checked, because C99
    /// 6.2.1p7 puts its name in scope from the end of its declarator and a
    /// static initialiser is where that matters most: `struct list head = {
    /// &head, &head }` is an address constant naming the object it
    /// initialises. It starts out with the zero C gives static storage, and
    /// [`Sema::initialize_static_object`] replaces that with the value the
    /// declaration wrote. `None` means there is nothing to give a value to —
    /// no initialiser, or a declaration that did not check out.
    #[allow(clippy::too_many_arguments)]
    fn declare_static_object(
        &mut self,
        name: &ast::Ident,
        ty: Ty,
        is_const: bool,
        is_static: bool,
        file_scope: bool,
        thread_local: bool,
        declarator: &ast::InitDeclarator,
    ) -> Option<ObjectId> {
        // C99 6.9.2: a file-scope declaration without an initialiser is a
        // *tentative* definition, and repeating one — or completing it with an
        // initialiser later — is perfectly ordinary C.
        // The composite type is what a second declaration leaves behind
        // (6.2.7p4), which is how `int j[]; int j[3];` ends up with a size.
        let composite = match self.declared_here(&name.name).cloned() {
            Some(Entry::Object(existing)) if file_scope => {
                let declared = self.visible_object_ty(existing);
                self.composite_object_ty(declared, ty)
            }
            _ => None,
        };
        if file_scope
            && let Some(composite) = composite
            && let Some(Entry::Object(existing)) = self.declared_here(&name.name).cloned()
        {
            self.retype_object(existing, composite, declarator);
            self.note_object_ty(existing, composite);
            // C11 6.7.1p3: if `_Thread_local` appears in any declaration of an
            // object it has to appear in every one. Letting the two disagree
            // would silently give the object whichever storage the *first*
            // declaration asked for.
            if thread_local != self.program.object(existing).storage.is_thread_local() {
                let previous = self.program.object(existing).range;
                self.error_note(
                    name.range,
                    format!(
                        "'{}' is declared '_Thread_local' here but not there; the specifier \
                         has to be on every declaration of an object",
                        name.name
                    ),
                    previous,
                    format!("previous declaration of '{}' is", name.name),
                );
                return None;
            }
            return self.complete_tentative_definition(name, existing, declarator, !is_static);
        }

        self.check_redefinition(name);
        if ty.is_error() {
            // The declaration was already reported; the name is registered so
            // that using it does not produce a second diagnostic, but there is
            // nothing to generate an item from.
            let id = self.new_object(&name.name, ty, Storage::Automatic, is_const, name.range);
            self.insert(&name.name, Entry::Object(id));
            return None;
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
        let exported = file_scope && !is_static;
        let storage = if thread_local {
            Storage::ThreadLocal {
                item_name,
                exported,
            }
        } else {
            Storage::Static {
                item_name,
                exported,
            }
        };
        let id = self.new_object(&name.name, ty, storage, is_const, name.range);
        self.insert(&name.name, Entry::Object(id));
        if file_scope {
            // An identifier with linkage carries the type of the declaration
            // it was named through rather than the object's; see
            // [`Scope::composites`].
            self.note_object_ty(id, ty);
        }

        // C zero-initialises static storage; an initialiser, if written, must
        // be a constant expression, and replaces the zero once it has been
        // checked.
        let init = self.zero(ty, declarator.range);
        if declarator.init.is_some() {
            self.initialized.insert(id);
        }
        self.program.statics.push(StaticVar { object: id, init });
        declarator.init.is_some().then_some(id)
    }

    /// The composite of two declarations of one object (C99 6.2.7p4), or
    /// `None` when the two types are not compatible at all.
    ///
    /// The one case where the composite is neither of the two spellings is an
    /// array: the composite of a completed array type and an incomplete one is
    /// the *completed* one, which is what makes `extern int j[]; int j[3];`
    /// declare one object whose size this unit knows.
    fn composite_object_ty(&mut self, declared: Ty, again: Ty) -> Option<Ty> {
        if declared == again {
            return Some(declared);
        }
        if !self.compatible(declared, again) {
            return None;
        }
        if self.types().is_incomplete_array(declared) {
            return Some(again);
        }
        Some(declared)
    }

    /// Gives an already-declared object the composite type a redeclaration
    /// left it with, and the zero of that type if it is waiting for one.
    fn retype_object(&mut self, id: ObjectId, ty: Ty, declarator: &ast::InitDeclarator) {
        if self.program.object(id).ty == ty {
            return;
        }
        self.program.objects[id.0 as usize].ty = ty;
        let zero = self.zero(ty, declarator.range);
        if !self.initialized.contains(&id)
            && let Some(entry) = self.program.statics.iter_mut().find(|s| s.object == id)
        {
            entry.init = zero;
        }
    }

    /// Gives an object with static storage duration the value its initialiser
    /// says, which C99 6.7.8p4 requires to be a constant expression.
    fn initialize_static_object(
        &mut self,
        id: ObjectId,
        declarator: &ast::InitDeclarator,
        init: Expr,
    ) {
        let ty = self.program.object(id).ty;
        let value = self
            .static_init(init, "initializer")
            .unwrap_or_else(|| self.zero(ty, declarator.range));
        if let Some(entry) = self.program.statics.iter_mut().find(|s| s.object == id) {
            entry.init = value;
        }
    }

    /// Handles a repeated file-scope declaration of the same object.
    ///
    /// `int n; int n = 1;` declares one object twice and initialises it once,
    /// which C allows; a second initialiser does not. The object to give the
    /// initialiser to comes back, or `None` when this declaration wrote none
    /// or is the second one that did.
    fn complete_tentative_definition(
        &mut self,
        name: &ast::Ident,
        id: ObjectId,
        declarator: &ast::InitDeclarator,
        defines: bool,
    ) -> Option<ObjectId> {
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
        declarator.init.as_ref()?;
        if !self.initialized.insert(id) {
            let previous = self.program.object(id).range;
            self.error_note(
                name.range,
                format!("redefinition of '{}'", name.name),
                previous,
                format!("previous definition of '{}' is", name.name),
            );
            return None;
        }
        Some(id)
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
        // `extern int j[];` is the canonical incomplete array type, and
        // `extern struct incomplete es;` (WG14 DR047) is the same rule for a
        // tag: the object is defined in another unit, so neither its size nor
        // the completeness of its type is any of this one's business
        // (6.2.5p22), and only its address is ever taken here.
        let Some(ty) = self.declared_object_ty_of(&declarator.ty, &name.name, Completeness::Any)
        else {
            return;
        };
        if self.types().is_vm(ty) {
            self.error(
                declarator.range,
                "a variable length array cannot have static storage duration",
            );
            return;
        }
        if let Some(init) = &declarator.init {
            self.error(
                init.range,
                format!(
                    "'{}' is declared 'extern' and cannot have an initializer here",
                    name.name
                ),
            );
        }
        // The declaration a redeclaration has to agree with is the one with
        // *linkage*, which lives at file scope: `int v = 4; { extern int v; }`
        // names the file-scope `v` and not the local one (C99 6.2.2p4), since
        // a block-scope object declared without `extern` has no linkage at
        // all.
        if let Some(Entry::Object(existing)) = self.lookup_linked(&name.name).cloned() {
            // The type to compose with is the one *visible here* rather than
            // the object's, which may already carry a composite an inner block
            // elsewhere gave it and which C would not have shown this
            // declaration (6.2.7p4).
            let declared = self.visible_object_ty(existing);
            if let Some(composite) = self.composite_object_ty(declared, ty) {
                self.retype_object(existing, composite, declarator);
                // The composite lasts to the end of *this* scope only, so a
                // `sizeof` after the block sees the outer declaration's type
                // again — WG14 DR011.
                self.note_object_ty(existing, composite);
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
        self.note_object_ty(id, ty);
    }

    // -- typedef ------------------------------------------------------------

    fn declare_typedef(
        &mut self,
        name: &ast::Ident,
        ty: &ast::Type,
        init: Option<&ast::Initializer>,
        attrs: &ast::Attributes,
    ) -> Vec<Stmt> {
        if let Some(init) = init {
            self.error(init.range, "a 'typedef' cannot have an initializer");
        }
        // A `typedef` *stores* the reason its type would not resolve and
        // reports it where the name is used, which is right for a type that is
        // merely unavailable here. `auto` in a parameter is a mistake in the
        // declaration itself and has no use to wait for, so it is reported
        // where it stands — `C23/n3007.c`'s `typedef void (*fp)(auto);`.
        if let Some(range) = auto_in_prototype(ty) {
            self.error(range, "'auto' is not allowed in a function prototype");
        }
        let file_scope = self.at_file_scope();
        // `typedef int A[n];` is a variably modified type, and C99 6.7.7p4
        // says the bound is evaluated *here*, once, however many objects `A`
        // later declares. That needs an object to keep the length in, which is
        // what `BoundMode::Object` asks for; at file scope there is no moment
        // at which the bound could be evaluated at all.
        let resolved = match self.resolve_declared_ty(ty, &name.name) {
            Ok(ty) => Ok(self.apply_mode(ty, attrs)),
            Err(err) if err.message.is_empty() => return Vec::new(),
            Err(err) => Err(err.message),
        };
        let mut out = self.take_vm_bounds();
        if let Ok(resolved) = resolved
            && self.types().is_vm(resolved)
            && file_scope
        {
            self.error(
                ty.range,
                format!("variably modified '{}' at file scope", name.name),
            );
            out.clear();
        }
        // `typedef struct { … } T __attribute__((aligned(N)));` asks for a
        // stricter alignment than the members give; see
        // `Sema::align_typedef_record`.
        if let Ok(resolved) = &resolved
            && let Some(aligned) = attrs.aligned.clone()
            && let Some(want) = self.alignment_of(Some(&aligned))
        {
            self.align_typedef_record(*resolved, want, aligned.range);
        }
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
            return out;
        }
        let Ok(resolved) = resolved else { return out };
        if self.claim_anonymous_tag(resolved, &name.name) {
            return out;
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
            return out;
        }
        let rust_name = self.reserve_item_name(&name.name);
        self.program.typedefs.push(TypedefItem {
            rust_name,
            ty: resolved,
            range: name.range,
        });
        out
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

    /// Evaluates the bounds of the array parameters of a definition, on entry
    /// and in declaration order (C99 6.9.1p10).
    ///
    /// Two things happen here. The *inner* dimensions of a variably modified
    /// parameter — the `m` of `void f(int n, int m, double a[n][m])` — are
    /// part of its adjusted type, `double (*)[m]`, and get a hidden object
    /// apiece, bound here, which is what makes `a[i][j]`, `sizeof a[0]` and
    /// `a + 1` mean anything in the body. The *outermost* one is not part of
    /// the type at all — the parameter is a pointer — so its value is thrown
    /// away and only its side effects are kept; a bound that plainly cannot do
    /// anything is left out, so that the overwhelmingly common
    /// `void f(int n, int a[n])` still generates nothing at all.
    fn parameter_size_effects(&mut self, func: &ast::FunctionType) -> Vec<Stmt> {
        // A compound literal written in a bound — `int g(char *p[f((int[27]){0})])`,
        // which is WG14 N2819's example — is an object of the function body's
        // outermost block, so its definition goes in front of everything, the
        // way `Sema::block_items` puts a block's own literals at its head.
        let enclosing = std::mem::take(&mut self.compound_literals);
        let mut out = Vec::new();
        for (index, param) in func.params.iter().enumerate() {
            // The bounds inside the declarator come first, innermost first,
            // which is the order GCC evaluates them in.
            if let Some(name) = &param.name {
                out.extend(self.variably_modified_param(param, index, &name.name));
            }
            let ast::TypeKind::Array {
                size: ast::ArraySize::Expr(size),
                ..
            } = &param.ty.kind
            else {
                continue;
            };
            let Some(value) = self.expr(size) else {
                continue;
            };
            let harmless = match &value.kind {
                ExprKind::Int(_) | ExprKind::Float(_) => true,
                ExprKind::Load(place) => matches!(place.kind, PlaceKind::Object(_)),
                _ => false,
            };
            if !harmless {
                out.push(Stmt::Expr(value));
            }
        }
        let literals = std::mem::replace(&mut self.compound_literals, enclosing);
        let mut prologue = Vec::with_capacity(literals.len() + out.len());
        for object in literals {
            let info = self.program.object(object);
            let (ty, range) = (info.ty, info.range);
            let init = self.zero(ty, range);
            prologue.push(Stmt::Let {
                object,
                init,
                explicit: false,
            });
        }
        prologue.append(&mut out);
        prologue
    }

    /// Re-resolves a definition's parameter type inside the *body's* scope,
    /// where its bounds can be evaluated and kept.
    ///
    /// The signature keeps the type the prototype gave it — a parameter's
    /// bound is not part of its type (C99 6.7.5.3p7), and any two variably
    /// modified types are compatible whatever their bounds — while the
    /// *object* the body sees is retyped with the lengths bound here. That is
    /// the whole of `void f(int n, int m, double a[n][m])`: `a` is a
    /// `double (*)[m]` whose `m` was read on entry and cannot change
    /// afterwards, however the body assigns to the parameter `m`.
    fn variably_modified_param(
        &mut self,
        param: &ast::ParamDecl,
        index: usize,
        name: &str,
    ) -> Vec<Stmt> {
        // Only an array or a pointer declarator can carry an inner bound, and
        // resolving anything else again would define a tag twice.
        if !matches!(
            param.ty.kind,
            ast::TypeKind::Array { .. } | ast::TypeKind::Pointer(_)
        ) {
            return Vec::new();
        }
        let Ok(ty) = self.resolve_param_ty_declared(&param.ty, name) else {
            // Whatever is wrong with it was reported when the signature was
            // built; this pass says nothing new.
            self.vm_bounds.clear();
            return Vec::new();
        };
        let bounds = self.take_vm_bounds();
        if bounds.is_empty() {
            return Vec::new();
        }
        // The parameter object was created from the signature's type; it is
        // the same type but for the bounds, which is exactly what has to
        // change.
        if let Some(object) = self.func_params.get(index).copied() {
            self.program.objects[object.0 as usize].ty = ty;
        }
        bounds
    }

    /// Resolves a parameter list, inside the prototype scope its names live
    /// in; `None` means the list was reported and there is no signature.
    fn declare_params(
        &mut self,
        func: &ast::FunctionType,
        definition: Option<&ast::FunctionDef>,
        param_tys: &mut Vec<Ty>,
        param_names: &mut Vec<Option<String>>,
    ) -> Option<()> {
        for param in &func.params {
            // A parameter is an object of automatic storage duration; neither
            // of these can apply to one.
            if let Some(storage) = &param.specifiers.storage
                && storage.node == ast::StorageClass::Constexpr
            {
                self.error(
                    storage.range,
                    format!("'{}' is not allowed on a parameter", storage.node.as_str()),
                );
            }
            if let Some(range) = param.specifiers.thread_local {
                self.error(range, "'_Thread_local' is not allowed on a parameter");
            }
            let ty = match self.resolve_param_ty(&param.ty) {
                Ok(ty) => ty,
                Err(err) => {
                    self.report_type_error(err);
                    return None;
                }
            };
            if ty.is_void() {
                // C99 6.7.5.3p10: a parameter list of one unnamed parameter of
                // type `void` is a prototype with *no* parameters. The parser
                // recognises the keyword spelling on its own; reaching `void`
                // through a `typedef` is DR157 part 1, and only sema can see
                // it — `typedef void V; int f(V);` is `int f(void)`.
                if func.params.len() == 1 && !func.variadic && param.name.is_none() {
                    param_tys.clear();
                    param_names.clear();
                    break;
                }
                self.error(param.range, "parameter has incomplete type 'void'");
                return None;
            }
            // C99 6.9.1p7: it is a *definition* whose parameters must have a
            // complete type. A declaration that is not one may mention a tag
            // this unit has not defined — `void f(struct S s);` before
            // `struct S` — because nothing here has to know its size; `drs`
            // DR103 is exactly that question, and GCC and Clang both accept
            // it with only a warning about the tag's scope.
            if definition.is_some() && !self.types().is_complete(ty) {
                self.error(
                    param.range,
                    format!("parameter has incomplete type '{}'", self.tyname(ty)),
                );
                return None;
            }
            // C23 N2480 lets a parameter of a *definition* go unnamed, exactly
            // as C++ always has; GCC and Clang accepted it before that and
            // warn about it only under `-pedantic`
            // ("ISO C does not support omitting parameter names in function
            // definitions before C23"). There is nothing to bind — the body
            // cannot name the parameter — but the generated item still needs
            // one in that position; see `Sema::function_def`.
            if definition.is_some() && param.name.is_none() {
                self.require_standard(
                    crate::Standard::C23,
                    "omitting a parameter name in a function definition",
                    param.range,
                );
            }
            // The name is visible to the declarators that follow it, and to
            // nothing else; the object exists only so that a bound written
            // there resolves to something.
            if let Some(name) = &param.name {
                let object = self.new_object(&name.name, ty, Storage::Automatic, false, name.range);
                self.insert(&name.name, Entry::Object(object));
            }
            param_tys.push(ty);
            param_names.push(param.name.as_ref().map(|n| n.name.clone()));
        }
        Some(())
    }

    /// Declares (but does not define) a function.
    ///
    /// `definition` carries the parameter declarations of a *definition*, which
    /// is what turns the check into "this is the definition". `scope` says
    /// whether the name belongs to the file scope, where C gives every
    /// function its linkage, or to the block it was written in, which is where
    /// GNU puts a [nested one](Sema::nested_function_def).
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
        scope: FuncScope,
    ) -> Option<FuncId> {
        let specifiers = &decl.specifiers;
        // A nested function is an item of this unit and never a symbol of its
        // own, whatever the unit's `#pragma cinrs export` says.
        let is_static = scope == FuncScope::Nested
            || specifiers.storage.as_ref().map(|s| s.node) == Some(ast::StorageClass::Static);
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
        self.reject_cleanup(attrs, "a function");
        // C23 has no `constexpr` functions, and neither has this: a constant
        // here is a value, folded wherever its name is used.
        if let Some(storage) = &specifiers.storage
            && storage.node == ast::StorageClass::Constexpr
        {
            let at = storage.range;
            self.error(at, "'constexpr' is not supported on a function");
        }

        // A definition's identifier list has already become a parameter list
        // (see `Sema::old_style_params`), so one that is still here belongs to
        // a declaration — where C99 6.7.5.3p3 says it must be empty.
        if let Some(first) = func.kr_names.first() {
            self.error(
                first.range,
                "an identifier list is only allowed in a function definition; \
                 a declaration needs the parameter types",
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
        // C99 6.2.1p4's *function prototype scope*: a parameter's name is
        // visible to the declarators that follow it, which is what makes the
        // bound of `void f(int n, double a[3][n])` resolve to the parameter
        // rather than to nothing. The names are gone again at the closing
        // parenthesis, and the bounds themselves are never evaluated here —
        // see [`BoundMode::Unevaluated`].
        self.push_prototype_scope();
        let result = self.declare_params(func, definition, &mut param_tys, &mut param_names);
        // A tag the list declared belongs to the prototype scope and is gone
        // with it — unless this list is a *definition*'s, where 6.2.1p4 gives
        // it the body's block scope instead; see [`Sema::param_tags`].
        self.pop_prototype_scope(definition.is_some());
        result?;

        if func.old_style {
            // C99 6.9.1p7: the definition's type has no prototype, so every
            // caller applies the default argument promotions — and the
            // generated item has to take what a caller really passes. The
            // declared types come back on entry; see `Sema::function_def`.
            for ty in &mut param_tys {
                *ty = ty.promote_argument(&self.target);
            }
        }
        let sig = Signature {
            ret,
            params: param_tys,
            variadic: func.variadic,
            prototyped: self.is_prototyped(func),
        };

        // A nested function has no linkage: its name lives in the block it was
        // written in, where it shadows whatever a file-scope declaration of the
        // same name means, and the only declaration it can be a redeclaration
        // of is one written in the same block — GNU's `auto int g(int);`
        // forward declaration.
        let visible = match scope {
            FuncScope::File => self.lookup(&name.name),
            FuncScope::Nested => self.declared_here(&name.name),
        };
        let existing = match visible {
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
                // A lifted nested function becomes a file-scope Rust item, so
                // it needs a name of its own: `__cinrs_<enclosing>_<name>`,
                // which reads as what it is and is made unique against
                // everything else the unit generates.
                let item_name = match scope {
                    FuncScope::File => None,
                    FuncScope::Nested => {
                        let base = format!("__cinrs_{}_{}", self.func_name, name.name);
                        Some(self.reserve_item_name(&base))
                    }
                };
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
                    uses_alloca: false,
                    body: None,
                    item_name,
                    env: Vec::new(),
                    range: name.range,
                });
                match scope {
                    FuncScope::File => {
                        self.item_names.insert(name.name.clone());
                        self.insert_at_file_scope(&name.name, Entry::Function(id));
                    }
                    FuncScope::Nested => self.insert(&name.name, Entry::Function(id)),
                }
                id
            }
        };
        Some(id)
    }

    /// The parameter list an old-style (K&R) definition really declares.
    ///
    /// `int f(a, b) int a; char *b; { … }` writes an *identifier list* and then
    /// a declaration list saying what each name is; C99 6.9.1p6 makes the two
    /// into the parameter list every other pass here expects, so sema builds
    /// it once, up front, and works on that.
    ///
    /// Two things survive the translation. The type still has **no prototype**
    /// (6.9.1p7): a call to it applies the default argument promotions and is
    /// only compatible with a prototype whose parameters are already their own
    /// promoted forms — which is why [`ast::FunctionType::old_style`] is set
    /// and why [`Sema::declare_function`] builds the signature out of the
    /// promoted types. And a name the declaration list leaves out is an `int`,
    /// which is implicit `int` and therefore C89's alone (a constraint
    /// violation from C99 on, and an error in GCC 14).
    fn old_style_params(
        &mut self,
        def: &ast::FunctionDef,
        func: &ast::FunctionType,
    ) -> ast::FunctionType {
        let mut out = func.clone();
        out.kr_names = Vec::new();
        out.old_style = true;
        out.has_prototype = false;

        if !self.gating.old_style_definitions() {
            self.error(
                def.ty.range,
                "old-style function definitions were removed in C23",
            );
        }
        if func.has_prototype {
            // `int f(int a) int a; { … }` — the declaration list has nothing
            // left to declare.
            if let Some(first) = def.kr_decls.first() {
                self.error(
                    first.range,
                    "a declaration list is not allowed after a parameter type list",
                );
            }
            out.old_style = false;
            out.has_prototype = true;
            return out;
        }

        // What the declaration list says, by name.
        let mut declared: Vec<(String, ast::ParamDecl)> = Vec::new();
        for decl in &def.kr_decls {
            if decl.declarators.is_empty() {
                self.error(decl.range, "this declaration declares no parameter");
                continue;
            }
            if let Some(storage) = &decl.specifiers.storage
                && storage.node != ast::StorageClass::Register
            {
                // `register` is the one C allows on a parameter, and the only
                // one a K&R declaration list is ever written with.
                self.error(
                    storage.range,
                    format!("'{}' is not allowed on a parameter", storage.node.as_str()),
                );
            }
            for declarator in &decl.declarators {
                let Some(name) = &declarator.name else {
                    self.error(declarator.range, "this declaration declares no parameter");
                    continue;
                };
                if declarator.init.is_some() {
                    self.error(
                        declarator.range,
                        format!("parameter '{}' cannot have an initializer", name.name),
                    );
                }
                if !func.kr_names.iter().any(|n| n.name == name.name) {
                    self.error(
                        name.range,
                        format!(
                            "declaration for parameter '{}', which is not in the \
                             identifier list",
                            name.name
                        ),
                    );
                    continue;
                }
                if declared.iter().any(|(other, _)| *other == name.name) {
                    self.error(
                        name.range,
                        format!("redefinition of parameter '{}'", name.name),
                    );
                    continue;
                }
                declared.push((
                    name.name.clone(),
                    ast::ParamDecl {
                        specifiers: decl.specifiers.clone(),
                        name: Some(name.clone()),
                        ty: declarator.ty.clone(),
                        range: declarator.range,
                    },
                ));
            }
        }

        let mut seen: Vec<&str> = Vec::new();
        for ident in &func.kr_names {
            if seen.contains(&ident.name.as_str()) {
                self.error(
                    ident.range,
                    format!("redefinition of parameter '{}'", ident.name),
                );
                continue;
            }
            seen.push(&ident.name);
            match declared.iter().find(|(name, _)| *name == ident.name) {
                Some((_, param)) => out.params.push(param.clone()),
                None => {
                    if !self.gating.implicit_int() {
                        self.error(
                            ident.range,
                            format!(
                                "type specifier missing for parameter '{}'; C99 does \
                                 not support implicit 'int'",
                                ident.name
                            ),
                        );
                    }
                    out.params.push(ast::ParamDecl {
                        specifiers: ast::DeclSpecifiers {
                            storage: None,
                            thread_local: None,
                            inline: false,
                            noreturn: None,
                            alignas: None,
                            attrs: ast::Attributes::default(),
                            base: implicit_int_type(ident.range),
                            range: ident.range,
                        },
                        name: Some(ident.clone()),
                        ty: implicit_int_type(ident.range),
                        range: ident.range,
                    });
                }
            }
        }
        out
    }

    pub(super) fn function_def(&mut self, def: &ast::FunctionDef) {
        self.check_function_def(def, FuncScope::File);
    }

    /// Checks GNU's nested function definition and lifts it out.
    ///
    /// The body is checked where it stands, so it sees the enclosing
    /// function's parameters and locals exactly as far as C's scoping makes
    /// them visible; every one of those it uses becomes a hidden pointer
    /// parameter (see [`ir::EnvParam`]), and the definition becomes a
    /// file-scope item that nothing else in the unit can name. What is left in
    /// the enclosing function is nothing at all: the name is in scope for the
    /// rest of its block, and a call to it passes the addresses of the objects
    /// it uses.
    pub(super) fn nested_function_def(&mut self, def: &ast::FunctionDef) {
        if self.nest.is_empty() {
            // A statement expression written in a file-scope initialiser is
            // the one block that is inside no function at all, and there is
            // nothing for a nested definition there to be nested in.
            self.error(
                def.name.range,
                format!(
                    "'{}' is defined inside a statement expression at file scope, which is \
                     inside no function; a nested function definition needs an enclosing one",
                    def.name.name
                ),
            );
            return;
        }
        if let Some(storage) = &def.specifiers.storage
            && storage.node != ast::StorageClass::Auto
        {
            self.error(
                storage.range,
                format!(
                    "'{}' is not allowed on a nested function definition: a nested function \
                     has no linkage, and 'auto' is the only storage class GNU C accepts on \
                     one",
                    storage.node.as_str()
                ),
            );
        }
        self.check_function_def(def, FuncScope::Nested);
    }

    fn check_function_def(&mut self, def: &ast::FunctionDef, scope: FuncScope) {
        let ast::TypeKind::Function(func) = &def.ty.kind else {
            return;
        };
        // An old-style definition is turned into an ordinary parameter list
        // here, once, so that nothing downstream has to know about identifier
        // lists; see [`Sema::old_style_params`].
        let synthesized;
        let func = if func.kr_names.is_empty() && def.kr_decls.is_empty() {
            func
        } else {
            synthesized = self.old_style_params(def, func);
            &synthesized
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
            scope,
        ) else {
            return;
        };
        // The enclosing function's state goes aside for the length of a nested
        // definition, and the nesting gains a level.
        let saved = (scope == FuncScope::Nested).then(|| self.save_function_state());
        self.nest.push(NestFrame {
            func: id,
            labels: HashSet::new(),
        });
        self.nest_chains
            .insert(id, self.nest.iter().map(|frame| frame.func).collect());
        let level = (self.nest.len() - 1) as u32;

        // Parameters live in the same scope as the body's outermost block, so
        // that `int f(int x) { int x; }` is the redefinition C says it is.
        self.push_scope();
        // And so do the tags the parameter list declared: a `struct T` written
        // in a *definition*'s list has block scope terminating with the body
        // (6.2.1p4), which is what makes `f(struct T { int a; } t)` able to
        // reach `t.a`.
        self.take_param_tags();
        self.ret_ty = self.program.function(id).sig.ret;
        self.func_name = def.name.name.clone();
        self.func_variadic = func.variadic;
        self.va_param = None;
        self.breakables.clear();
        self.switch_stack.clear();
        self.vla_scopes.clear();
        self.switch_vla_depths.clear();
        self.goto_scopes.clear();
        self.label_vla_scopes.clear();
        self.func_uses_alloca = false;
        self.next_loop = 0;
        self.next_switch = 0;
        self.next_label = 0;
        // Whether the body can keep Rust's own control flow is decided before
        // it is checked, because it changes how `switch` is lowered.
        self.cfg_mode = Self::needs_cfg(&def.body);
        self.collect_labels(&def.body);
        // A `goto` in a function nested inside this one that names one of
        // these labels is GNU's nonlocal goto, which is refused by name rather
        // than as "no such label".
        if let Some(frame) = self.nest.last_mut() {
            frame.labels = self.labels.keys().cloned().collect();
        }

        let mut params = Vec::with_capacity(func.params.len());
        // The old-style parameters whose declared type is not what the ABI
        // hands over: the item takes the promoted one under a hidden name, and
        // the prologue below binds the C name to the declared type.
        let mut converted: Vec<(ast::Ident, Ty, Ty, ObjectId, bool)> = Vec::new();
        for (index, (param, ty)) in func
            .params
            .iter()
            .zip(self.program.function(id).sig.params.clone())
            .enumerate()
        {
            let Some(name) = &param.name else {
                // C23's unnamed parameter (N2480). Nothing in the body can
                // reach it, but the generated item still takes an argument in
                // that position, so it gets a name of its own — the parameter
                // list has to keep the shape the signature has.
                let object = self.new_object(
                    &format!("__cinrs_unnamed_param{index}"),
                    ty,
                    Storage::Automatic,
                    param.ty.qualifiers.is_const,
                    param.range,
                );
                params.push(object);
                continue;
            };
            if func.old_style {
                // `resolve_param_ty` already succeeded for this parameter in
                // `declare_function`, or there would be no `id` to be here
                // with; it neither reports nor changes anything.
                let declared = self.resolve_param_ty(&param.ty).unwrap_or(ty);
                if declared != ty {
                    let object = self.new_object(
                        &format!("__cinrs_kr_{}", name.name),
                        ty,
                        Storage::Automatic,
                        false,
                        name.range,
                    );
                    converted.push((
                        name.clone(),
                        declared,
                        ty,
                        object,
                        param.ty.qualifiers.is_const,
                    ));
                    params.push(object);
                    continue;
                }
            }
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
        // `let a: c_char = __cinrs_kr_a as c_char;` — C99 6.9.1p10 gives an
        // old-style parameter the type its own declaration gave it, while the
        // caller passed the promoted one, which is what the item takes.
        let mut body = Vec::new();
        for (name, declared, promoted, object, is_const) in converted {
            let load = Expr::new(
                ExprKind::Load(super::place_of(
                    PlaceKind::Object(object),
                    promoted,
                    false,
                    name.range,
                )),
                promoted,
                name.range,
            );
            let init = self.convert(load, declared);
            let local = self.new_object(
                &name.name,
                declared,
                Storage::Automatic,
                is_const,
                name.range,
            );
            self.insert(&name.name, Entry::Object(local));
            body.push(Stmt::Let {
                object: local,
                init,
                explicit: true,
            });
        }
        // C99 6.9.1p10 again: the size expressions of a variably modified
        // parameter are evaluated on entry. The bound is not part of the
        // adjusted type — the parameter is a pointer — but `void f(int n, int
        // a[n++])` still increments `n`, and a definition is the one place
        // where that is observable.
        body.extend(self.parameter_size_effects(func));
        body.extend(self.block_items(&def.body.items));
        // A forward `goto` names a label the walk above had not reached yet, so
        // the jumps are checked now that every label's scope is known.
        self.check_goto_vla_scopes();
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
        // A nested function's own objects are inside this range too, and they
        // are its locals rather than this one's; the level each object was
        // created at is what separates them. So are the hidden environment
        // parameters, which are parameters and not `let` bindings.
        let hidden: Vec<ObjectId> = self
            .program
            .function(id)
            .env
            .iter()
            .map(|entry| entry.param)
            .collect();
        let locals: Vec<ObjectId> = (first_object..last_object)
            .map(ObjectId)
            .filter(|object| {
                self.program.object(*object).storage == Storage::Automatic
                    && self.object_level(*object) == level as usize
                    && !hidden.contains(object)
            })
            .collect();
        let entry = &mut self.program.functions[id.0 as usize];
        entry.params = params;
        entry.locals = locals;
        entry.uses_alloca = self.func_uses_alloca;
        entry.body = Some(body);
        self.nest.pop();
        if let Some(saved) = saved {
            self.restore_function_state(saved);
        }
    }

    /// Puts the state of the function being checked aside, so that a nested
    /// definition can be checked in the middle of it.
    fn save_function_state(&mut self) -> Box<SavedFunc> {
        Box::new(SavedFunc {
            ret_ty: self.ret_ty,
            func_name: std::mem::take(&mut self.func_name),
            func_variadic: self.func_variadic,
            func_params: std::mem::take(&mut self.func_params),
            va_param: self.va_param.take(),
            cfg_mode: self.cfg_mode,
            labels: std::mem::take(&mut self.labels),
            breakables: std::mem::take(&mut self.breakables),
            switch_stack: std::mem::take(&mut self.switch_stack),
            vla_scopes: std::mem::take(&mut self.vla_scopes),
            label_vla_scopes: std::mem::take(&mut self.label_vla_scopes),
            goto_scopes: std::mem::take(&mut self.goto_scopes),
            switch_vla_depths: std::mem::take(&mut self.switch_vla_depths),
            func_uses_alloca: self.func_uses_alloca,
            // A `cleanup` owed by the enclosing block is not owed by the
            // nested function's `return`.
            cleanup_depth: std::mem::take(&mut self.cleanup_depth),
            next_loop: self.next_loop,
            next_switch: self.next_switch,
            next_label: self.next_label,
        })
    }

    fn restore_function_state(&mut self, saved: Box<SavedFunc>) {
        let saved = *saved;
        self.ret_ty = saved.ret_ty;
        self.func_name = saved.func_name;
        self.func_variadic = saved.func_variadic;
        self.func_params = saved.func_params;
        self.va_param = saved.va_param;
        self.cfg_mode = saved.cfg_mode;
        self.labels = saved.labels;
        self.breakables = saved.breakables;
        self.switch_stack = saved.switch_stack;
        self.vla_scopes = saved.vla_scopes;
        self.label_vla_scopes = saved.label_vla_scopes;
        self.goto_scopes = saved.goto_scopes;
        self.switch_vla_depths = saved.switch_vla_depths;
        self.func_uses_alloca = saved.func_uses_alloca;
        self.cleanup_depth = saved.cleanup_depth;
        self.next_loop = saved.next_loop;
        self.next_switch = saved.next_switch;
        self.next_label = saved.next_label;
    }

    // -- static initialisers ------------------------------------------------

    /// Reduces an initialiser for an object with static storage duration to
    /// something the generated `static mut` item can hold.
    pub(super) fn static_init(&mut self, expr: Expr, what: &str) -> Option<Expr> {
        let (ty, range) = (expr.ty, expr.range);
        match expr.kind {
            // `int i = (1, 2);` — a comma operator is not part of a *constant
            // expression* (6.6p3), and the initialiser of an object with
            // static storage duration is supposed to be one. Both GCC and
            // Clang take it all the same, and only `-pedantic-errors` refuses
            // it, so the value of the right operand is the initialiser here
            // too — as long as the left one is itself a constant, since
            // nothing here could evaluate a call. WG14 DR035 (`drs/dr0xx.c`).
            //
            // Where C asks for an *integer constant expression* — an
            // enumerator, an array bound, a `case` label, `_Static_assert` —
            // the comma is still refused, which is what GCC does there
            // ("enumerator value for 'e' is not an integer constant") and what
            // this crate's strict entry points do everywhere.
            ExprKind::Comma { lhs, rhs } if self.const_eval(&lhs).is_some() => {
                self.static_init(*rhs, what)
            }
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
                // `(char *) 1 + 2` is a constant, but a Rust `.offset()` on a
                // pointer with no provenance is not: the arithmetic is done
                // here instead, and what is left is one integer cast.
                if let Some(folded) = self.fold_integer_pointer(&expr) {
                    return Some(folded);
                }
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

    /// Folds a pointer value built entirely out of integer constants into one
    /// integer cast to the pointer type, or `None` when it is not one.
    ///
    /// `(unsigned int *) 0xa000` needs nothing, but `(char *) 1 + 2` would
    /// become a `.offset(2)` on a pointer with no provenance, which is a
    /// const-evaluation error in Rust rather than an address. Doing the
    /// arithmetic here leaves one cast, which is a constant everywhere.
    fn fold_integer_pointer(&mut self, expr: &Expr) -> Option<Expr> {
        let ty = expr.ty;
        if !ty.is_pointer() {
            return None;
        }
        let value = self.integer_pointer_value(expr)?;
        let size = self.size_ty();
        let base = Expr::int(value, size, expr.range);
        Some(Expr::new(ExprKind::Cast(Box::new(base)), ty, expr.range))
    }

    /// The address an all-integer pointer expression names, in bytes.
    ///
    /// The address of a *place* is one of these when the place is reached from
    /// a constant pointer rather than from an object: `&((struct s *)0)->m` is
    /// the hand-written `offsetof` half the world's C still defines, and GCC
    /// folds it to the member's offset. See [`Sema::place_offset`].
    pub(super) fn integer_pointer_value(&mut self, expr: &Expr) -> Option<i128> {
        match &expr.kind {
            ExprKind::Int(v) => Some(*v),
            ExprKind::Zeroed if expr.ty.is_pointer() => Some(0),
            ExprKind::Cast(inner) if inner.ty.is_integer() || inner.ty.is_pointer() => {
                self.integer_pointer_value(inner)
            }
            ExprKind::AddrOf(place) if self.gating.dialect.is_gnu() => {
                let place = place.clone();
                self.place_offset(&place)
            }
            ExprKind::PtrOffset { ptr, index, sub } => {
                // The base has to be an integer, or this is an ordinary
                // address constant and `is_address_constant` will take it.
                let base = self.integer_pointer_value(ptr)?;
                let ExprKind::Int(count) = index.kind else {
                    return None;
                };
                let pointee = self.pointee(ptr.ty)?;
                let size = i128::from(self.size_of(pointee).unwrap_or(1));
                let delta = count.checked_mul(size)?;
                if *sub {
                    base.checked_sub(delta)
                } else {
                    base.checked_add(delta)
                }
            }
            _ => None,
        }
    }

    /// The constant address of a *place*, in bytes, when it has one.
    ///
    /// A place has one only when it is reached from a pointer that is itself a
    /// constant — which is exactly the shape of the `offsetof` every C program
    /// wrote before `<stddef.h>` had one:
    ///
    /// ```c
    /// #define offsetof(T, m) ((size_t) &((T *) 0)->m)
    /// ```
    ///
    /// GCC folds it, calls the folding an extension, and rejects it under
    /// `-pedantic-errors`; the GNU dialects do the same here and the strict
    /// entry points keep the "not a compile-time constant expression" error.
    /// A named object is deliberately absent — its address is the linker's
    /// answer, not one this can give — and so is a bit-field, which has no
    /// address at all.
    fn place_offset(&mut self, place: &Place) -> Option<i128> {
        match &place.kind {
            PlaceKind::Deref(ptr) => self.integer_pointer_value(ptr),
            PlaceKind::Field {
                base,
                record,
                index,
            } => {
                let field = self.types().record(*record).fields.get(*index)?;
                if field.bits.is_some() {
                    return None;
                }
                let offset = i128::from(field.offset);
                let base = base.as_ref().clone();
                self.place_offset(&base)?.checked_add(offset)
            }
            PlaceKind::Index { base, index } => {
                let ExprKind::Int(count) = index.kind else {
                    return None;
                };
                let pointee = self.pointee(base.ty)?;
                let size = i128::from(self.size_of(pointee).unwrap_or(1));
                let base = self.integer_pointer_value(base)?;
                base.checked_add(count.checked_mul(size)?)
            }
            _ => None,
        }
    }

    /// Whether a pointer value is one the linker can work out: a null pointer,
    /// the address of something with static storage duration, or a constant
    /// offset from one.
    ///
    /// An *integer* constant converted to a pointer is here too. C11 6.6p9's
    /// list of address constants does not have it, but 6.6p10 lets an
    /// implementation accept other forms of constant expression and every one
    /// does: `(unsigned int *) 0xa000` is how a program names a memory-mapped
    /// register, and Rust's `0xa000 as *mut u32` is a constant expression as
    /// well. `execute/20021010-2` and `execute/pr23324` are the two in the
    /// torture suite.
    fn is_address_constant(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Zeroed | ExprKind::FuncAddr(_) | ExprKind::Int(_) => true,
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
                let storage = &self.program.object(*id).storage;
                // The address of a thread-local object is not a link-time
                // constant: there is one per thread, and no thread exists yet
                // when a static initialiser is evaluated. GCC says the same.
                !matches!(storage, Storage::Automatic) && !storage.is_thread_local()
            }
            PlaceKind::Str(_) => true,
            // `__real__ g` for a file-scope `g` is as static as `g` itself.
            PlaceKind::Field { base, .. } | PlaceKind::ComplexPart { base, .. } => {
                self.is_static_place(base)
            }
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

/// Whether a declarator's type is one only its *initialiser* can complete.
///
/// C23's `auto x = e;` (and GNU's `__auto_type`) takes the whole type from the
/// initialiser, and `T x[] = { … }` takes the array's length from it. Those
/// two are checked together by [`Sema::typed_initializer`]; every other form
/// resolves its type from the declarator alone, so that the object can be in
/// scope for its own initialiser (C99 6.2.1p7). See [`Sema::declarator`].
fn type_from_initializer(declarator: &ast::InitDeclarator) -> bool {
    match &declarator.ty.kind {
        ast::TypeKind::Auto => true,
        // `auto *p = &a;` — the deduction happens through the declarator; see
        // [`auto_pointer_depth`].
        ast::TypeKind::Pointer(_) => {
            auto_pointer_depth(&declarator.ty).is_some() && declarator.init.is_some()
        }
        // `T x[] = { … }` takes its length from the list; `T x[];` with no
        // list at all is an *incomplete* array type, which is a declaration
        // rather than a mistake in the two places C99 6.9.2 allows it.
        ast::TypeKind::Array {
            size: ast::ArraySize::Unspecified,
            ..
        } => declarator.init.is_some(),
        _ => false,
    }
}

/// How many `*` stand between `auto` and the identifier, when the declarator
/// is nothing but a run of them.
///
/// C23 6.7.1p? asks the declarator of an inferred declaration to be "a plain
/// identifier, possibly with attributes", and GCC says so ("'auto' requires a
/// plain identifier ... as declarator"). Clang takes the pointer forms as a
/// documented extension — `auto *ptr = &a;` deduces `int` from an `int *`
/// initialiser and gives `ptr` the initialiser's own type — and `cinrs` takes
/// them too, exactly as it takes GCC's own extensions in every entry point.
/// `C23/n3007.c` is where both halves are checked: the deduction has to work,
/// and `auto *ptr2 = a;` with an `int` initialiser has to be refused.
///
/// `None` for anything that is not `auto` under a run of pointer derivations,
/// including a plain `auto` (which is [`ast::TypeKind::Auto`] and needs none
/// of this) and an array or function declarator, which C23 refuses outright.
fn auto_pointer_depth(ty: &ast::Type) -> Option<usize> {
    match &ty.kind {
        ast::TypeKind::Pointer(inner) => match &inner.kind {
            ast::TypeKind::Auto => Some(1),
            _ => auto_pointer_depth(inner).map(|depth| depth + 1),
        },
        _ => None,
    }
}

/// Where `auto` was written inside a function prototype, if it was.
///
/// C23 has it only on a declaration with an initialiser, so a parameter cannot
/// have it; a `typedef` stores the failure to resolve its type rather than
/// reporting it — the diagnostic belongs where the name is *used* — and this
/// is the one shape whose mistake is in the declaration itself, which is what
/// `C23/n3007.c`'s `typedef void (*fp)(auto);` asks about.
fn auto_in_prototype(ty: &ast::Type) -> Option<SourceRange> {
    match &ty.kind {
        ast::TypeKind::Pointer(inner) => auto_in_prototype(inner),
        ast::TypeKind::Array { elem, .. } => auto_in_prototype(elem),
        ast::TypeKind::Function(func) => func
            .params
            .iter()
            .find_map(|param| {
                matches!(param.ty.kind, ast::TypeKind::Auto)
                    .then_some(param.ty.range)
                    .or_else(|| auto_in_prototype(&param.ty))
            })
            .or_else(|| auto_in_prototype(&func.ret)),
        _ => None,
    }
}

/// The `int` a parameter no declaration list entry named has (C89 6.5.2).
fn implicit_int_type(range: SourceRange) -> ast::Type {
    ast::Type::plain(
        ast::TypeKind::Int {
            sign: ast::Sign::Signed,
            size: ast::IntSize::Int,
        },
        range,
    )
}
