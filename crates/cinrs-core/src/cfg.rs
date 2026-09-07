//! The control-flow-graph lowering, used for functions that jump.
//!
//! Nearly every C function's control flow maps onto Rust's own — that is what
//! [`codegen`](crate::codegen) does by default, and it is what makes an
//! expansion readable. Three constructs do not map:
//!
//! * `goto`, which may jump anywhere in the function, forwards or backwards,
//!   into or out of any block;
//! * a `case` (or `default`) label that is not a direct child of its `switch`
//!   body — Duff's device, where the labels sit inside a loop the `switch`
//!   wraps; and
//! * GNU's `&&label` and the computed `goto *e` it feeds, whose *value* is the
//!   state number the label's block was given.
//!
//! A function containing any of them is lowered here instead: its whole body becomes
//! a list of [basic blocks](BasicBlock) — straight-line statements ending in a
//! [`Terminator`] — which codegen emits as a state machine:
//!
//! ```text
//! let mut __cinrs_state: u32 = 0;
//! 'cfg: loop {
//!     match __cinrs_state {
//!         0 => { …; __cinrs_state = 3; continue 'cfg; }
//!         1 => { if c { __cinrs_state = 2; } else { __cinrs_state = 4; } continue 'cfg; }
//!         2 => { return x; }
//!         _ => ::core::unreachable!(),
//!     }
//! }
//! ```
//!
//! Every C construct becomes an edge: a loop is a block that branches to its
//! body or past it, `break` and `continue` are jumps to blocks the loop
//! registered, a `switch` is one [`Terminator::Switch`] whose table the `case`
//! labels fill in wherever they are found, and `goto` is a jump like any other.
//! A computed `goto` is a [`Terminator::IndirectJump`], whose successors are
//! the blocks of every label the function took the address of. Expressions are
//! untouched — codegen emits them exactly as it does in the structured mode.
//!
//! # Hoisting and renaming
//!
//! Rust has no way to jump over a `let`, so every local of the function is
//! defined once at the top, zero-initialised, and the declaration itself
//! becomes an assignment where it was written (only when the source really
//! wrote an initialiser — see [`Stmt::Let`]). C allows the same name in
//! sibling and nested blocks, so the hoisted locals need names that do not
//! collide: sema has already resolved each declaration to a distinct
//! [`ObjectId`], and this pass gives each one a unique Rust name derived from
//! the C one (`x`, `x_1`, …), which is the single place the renaming happens.
//! Objects with static storage duration are not hoisted at all; they are
//! separate items already.
//!
//! # Cleanups
//!
//! Hoisting is also why `__attribute__((cleanup(f)))` cannot be a drop guard
//! here: the local it would hang on lives to the end of the function, so the
//! guard would run once, and far too late. The registration
//! ([`ir::Stmt::Cleanup`]) is read as "from here to the end of this scope,
//! this call is owed", and every edge that *leaves* a scope emits what it owes
//! — innermost first — before it jumps: the bottom of a block, `break`,
//! `continue`, `return` and a `goto` whose label is outside. That is also the
//! only way to run a cleanup once per pass through a loop body. A forward
//! `goto` names a label this pass has not reached yet, so how deep each label
//! stands is collected up front, by the same walk over the same tree.
//!
//! [`ir::Stmt::Cleanup`]: crate::ir::Stmt::Cleanup
//!
//! # Readability
//!
//! A naive lowering produces a state per statement, which is unreadable. Three
//! cheap cleanups run before codegen sees the graph, in this order:
//!
//! 1. **Jump threading.** A block with no statements that only jumps somewhere
//!    else is removed and every edge into it re-pointed at its target. This is
//!    what keeps `case 0: case 1:` from costing a state of its own.
//! 2. **Chain merging.** A block whose only predecessor ends in an
//!    unconditional jump to it is appended to that predecessor, which is what
//!    keeps straight-line code in one arm.
//! 3. **Reverse-postorder numbering**, so the states read top to bottom, with
//!    unreachable blocks dropped on the way.
//!
//! All three leave a **pinned** block alone: one a `&&label` named has a number
//! the program computed, so it is never threaded past, never merged into a
//! predecessor and never dropped — a computed `goto` can still enter it even
//! when no edge does. [`Cfg::labels`] is what the numbers come back as.
//!
//! There is deliberately no relooper: the point is a correct, obvious lowering
//! of the functions that need it, not to reconstruct the loops of a function
//! that never had to leave the structured form.

use std::collections::{HashMap, HashSet};

use crate::capture::SourceRange;
use crate::ir::{
    BreakTarget, CaseRange, Expr, ExprKind, LabelId, LoopId, Object, ObjectId, Place, PlaceKind,
    Stmt, Storage, SwitchId, is_always_true,
};

/// Identifies a basic block inside a [`Cfg`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct BlockId(pub u32);

impl BlockId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

/// A local variable hoisted to the top of the function.
#[derive(Clone, Debug)]
pub struct Local {
    /// The object it defines.
    pub object: ObjectId,
    /// The Rust name it is given, unique within the function.
    pub rust_name: String,
}

/// A straight-line run of statements ending in a [`Terminator`].
#[derive(Clone, Debug)]
pub struct BasicBlock {
    /// Statements with no control flow of their own: [`ir::Stmt::Expr`],
    /// [`ir::Stmt::Nop`] and [`ir::Stmt::Vla`], whose three bindings are
    /// hoisted like every other local and whose allocation stays here.
    ///
    /// [`ir::Stmt::Expr`]: crate::ir::Stmt::Expr
    /// [`ir::Stmt::Nop`]: crate::ir::Stmt::Nop
    /// [`ir::Stmt::Vla`]: crate::ir::Stmt::Vla
    pub stmts: Vec<Stmt>,
    /// How the block ends.
    pub term: Terminator,
}

/// How a [`BasicBlock`] ends.
#[derive(Clone, Debug)]
pub enum Terminator {
    /// Control moves to another block.
    Jump {
        /// The block entered.
        target: BlockId,
        /// The construct the edge came from.
        range: SourceRange,
    },
    /// Control moves to one of two blocks depending on a condition.
    Branch {
        /// The controlling expression.
        cond: Expr,
        /// Entered when `cond` is non-zero.
        then_blk: BlockId,
        /// Entered otherwise.
        else_blk: BlockId,
    },
    /// GNU's computed `goto *e`: control moves to the block whose *number* the
    /// pointer holds.
    ///
    /// The blocks it can enter are the ones of every label whose address the
    /// function took, which is what keeps them reachable, numbered and
    /// unmerged; see [`Cfg::labels`]. The generated code stores the number in
    /// the state variable and goes round the dispatch again, so a value that
    /// is not one of them lands on the `unreachable!()` arm — which is the
    /// undefined behaviour C already had.
    IndirectJump {
        /// The pointer jumped through.
        target: Expr,
        /// Every block a label's address could name.
        blocks: Vec<BlockId>,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// A `switch` dispatch.
    Switch {
        /// The controlling expression, after the integer promotions.
        value: Expr,
        /// The `case` labels and the blocks they enter, in source order.
        cases: Vec<(CaseRange, BlockId)>,
        /// Where `default:` goes — past the statement when there is none.
        default: BlockId,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// The function returns.
    Return {
        /// The returned value.
        value: Option<Expr>,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// Control never gets here.
    ///
    /// Only reachable through a bug in this module, and emitted as
    /// `unreachable!()` so that such a bug is loud rather than silent.
    Unreachable,
}

impl Terminator {
    /// The blocks control can move to, in the order they should be read in.
    fn successors(&self) -> Vec<BlockId> {
        match self {
            Terminator::Jump { target, .. } => vec![*target],
            Terminator::Branch {
                then_blk, else_blk, ..
            } => vec![*then_blk, *else_blk],
            Terminator::Switch { cases, default, .. } => {
                let mut out: Vec<BlockId> = cases.iter().map(|(_, blk)| *blk).collect();
                out.push(*default);
                out
            }
            Terminator::IndirectJump { blocks, .. } => blocks.clone(),
            Terminator::Return { .. } | Terminator::Unreachable => Vec::new(),
        }
    }

    /// Applies `f` to every block this terminator names.
    fn map_targets(&mut self, mut f: impl FnMut(BlockId) -> BlockId) {
        match self {
            Terminator::Jump { target, .. } => *target = f(*target),
            Terminator::Branch {
                then_blk, else_blk, ..
            } => {
                *then_blk = f(*then_blk);
                *else_blk = f(*else_blk);
            }
            Terminator::Switch { cases, default, .. } => {
                for (_, blk) in cases.iter_mut() {
                    *blk = f(*blk);
                }
                *default = f(*default);
            }
            Terminator::IndirectJump { blocks, .. } => {
                for blk in blocks.iter_mut() {
                    *blk = f(*blk);
                }
            }
            Terminator::Return { .. } | Terminator::Unreachable => {}
        }
    }
}

/// A function body lowered into basic blocks.
#[derive(Clone, Debug)]
pub struct Cfg {
    /// Every local of the function, defined once at the top.
    pub locals: Vec<Local>,
    /// The blocks, in the order they should be emitted; block 0 is the entry.
    pub blocks: Vec<BasicBlock>,
    /// The block each label whose address was taken stands for, after the
    /// numbering — which is the value GNU's `&&label` has.
    ///
    /// Only those labels are in it: every other one may be threaded away or
    /// merged into its predecessor, which is what keeps an ordinary `goto`
    /// readable. See [`ir::ExprKind::LabelAddr`].
    ///
    /// [`ir::ExprKind::LabelAddr`]: crate::ir::ExprKind::LabelAddr
    pub labels: HashMap<LabelId, BlockId>,
}

/// Lowers a checked function body into a control-flow graph.
///
/// `params` are the function's parameters, whose names the hoisted locals must
/// not collide with, `objects` is the program's object table, and `pinned` are
/// the labels a `&&label` took the address of — in a fixed order, since it
/// decides the numbering of blocks nothing else reaches.
///
/// The body must end in a statement that always terminates — sema appends a
/// `return` when the source does not — so that no block falls off the end.
pub fn lower(body: Vec<Stmt>, params: &[ObjectId], objects: &[Object], pinned: &[LabelId]) -> Cfg {
    let mut used: HashSet<String> = params
        .iter()
        .map(|id| objects[id.0 as usize].name.clone())
        .collect();
    used.insert("__cinrs_state".to_owned());

    let mut lowerer = Lowerer {
        objects,
        blocks: Vec::new(),
        current: None,
        labels: HashMap::new(),
        loops: HashMap::new(),
        switches: HashMap::new(),
        locals: Vec::new(),
        used,
        cleanups: Vec::new(),
        label_cleanups: HashMap::new(),
        pinned: Vec::new(),
    };
    // A `goto` has to know how many cleanups its *target* is inside, and a
    // forward one names a label the walk below has not reached yet.
    lowerer.collect_label_cleanups(&body, 0);
    // The blocks of the address-taken labels are made first, so that they are
    // known before any of them is mentioned: every one of them is a possible
    // target of every computed `goto`, and none may be merged away.
    for id in pinned {
        let block = lowerer.label_block(*id);
        lowerer.pinned.push((*id, block));
    }
    let entry = lowerer.new_block();
    lowerer.current = Some(entry);
    lowerer.stmts(body);
    // Sema guarantees a trailing `return`, so anything still open here can
    // never be entered.
    if let Some(open) = lowerer.current.take() {
        lowerer.blocks[open.index()].term = Terminator::Unreachable;
    }
    lowerer.finish(entry)
}

/// What a loop's `break` and `continue` jump to, and how many cleanups each of
/// the two edges leaves behind.
#[derive(Clone, Copy)]
struct LoopBlocks {
    brk: BlockId,
    brk_cleanups: usize,
    cont: BlockId,
    cont_cleanups: usize,
}

/// A `switch` whose body is being walked.
struct SwitchFrame {
    brk: BlockId,
    brk_cleanups: usize,
    cases: Vec<(CaseRange, BlockId)>,
    default: Option<BlockId>,
}

struct Lowerer<'a> {
    objects: &'a [Object],
    blocks: Vec<BasicBlock>,
    /// The block statements are being appended to, if control can reach one.
    current: Option<BlockId>,
    labels: HashMap<LabelId, BlockId>,
    loops: HashMap<LoopId, LoopBlocks>,
    switches: HashMap<SwitchId, SwitchFrame>,
    locals: Vec<Local>,
    used: HashSet<String>,
    /// The `cleanup` calls owed by the scopes enclosing the statement being
    /// lowered, innermost last.
    ///
    /// A hoisted local lives to the end of the function, so there is no scope
    /// left for a drop guard to hang on; every edge that leaves a scope emits
    /// the calls it owes instead — which is also the only way to run a
    /// cleanup once per pass through a loop body.
    cleanups: Vec<Expr>,
    /// How many cleanups are owed where each label stands, from the pre-pass
    /// over the same tree; see [`Lowerer::collect_label_cleanups`].
    label_cleanups: HashMap<LabelId, usize>,
    /// The labels a `&&label` took the address of, and the blocks they stand
    /// for, in the order they were given.
    ///
    /// A pinned block keeps its identity through every clean-up pass — it is
    /// never threaded past, never merged into a predecessor and never dropped
    /// as unreachable — because its *number* is a value the program computed.
    pinned: Vec<(LabelId, BlockId)>,
}

impl Lowerer<'_> {
    // -- building blocks ----------------------------------------------------

    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(BasicBlock {
            stmts: Vec::new(),
            term: Terminator::Unreachable,
        });
        id
    }

    /// The block statements go into, starting a fresh one if the last one has
    /// already been terminated.
    fn current(&mut self) -> BlockId {
        match self.current {
            Some(id) => id,
            None => {
                let id = self.new_block();
                self.current = Some(id);
                id
            }
        }
    }

    fn push(&mut self, stmt: Stmt) {
        let id = self.current();
        self.blocks[id.index()].stmts.push(stmt);
    }

    /// Ends the current block, leaving control nowhere until a caller says
    /// where it continues.
    fn terminate(&mut self, term: Terminator) {
        let id = self.current();
        self.blocks[id.index()].term = term;
        self.current = None;
    }

    fn jump(&mut self, target: BlockId, range: SourceRange) {
        self.terminate(Terminator::Jump { target, range });
    }

    fn continue_at(&mut self, block: BlockId) {
        self.current = Some(block);
    }

    /// The block a label stands for, created the first time it is mentioned so
    /// that a forward `goto` works.
    fn label_block(&mut self, id: LabelId) -> BlockId {
        if let Some(block) = self.labels.get(&id) {
            return *block;
        }
        let block = self.new_block();
        self.labels.insert(id, block);
        block
    }

    // -- cleanups -----------------------------------------------------------

    /// Records how many `cleanup` calls are owed where each label stands.
    ///
    /// It is the same walk [`Lowerer::stmt`] makes, over the same tree, so the
    /// two agree by construction; doing it ahead of time is what lets a
    /// forward `goto` know how many scopes it leaves. A jump *into* the scope
    /// of a cleanup is not refused — GCC allows it, and runs the cleanup at
    /// the end of the scope all the same, which is what a lexical count does
    /// here too.
    fn collect_label_cleanups(&mut self, stmts: &[Stmt], depth: usize) {
        let mut depth = depth;
        for stmt in stmts {
            match stmt {
                Stmt::Cleanup(_) => depth += 1,
                Stmt::Block(items) => self.collect_label_cleanups(items, depth),
                Stmt::Label { id, body, .. } => {
                    self.label_cleanups.insert(*id, depth);
                    self.collect_label_cleanups(std::slice::from_ref(body), depth);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    self.collect_label_cleanups(std::slice::from_ref(then_branch), depth);
                    if let Some(branch) = else_branch {
                        self.collect_label_cleanups(std::slice::from_ref(branch), depth);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    self.collect_label_cleanups(std::slice::from_ref(body), depth);
                }
                Stmt::For { init, body, .. } => {
                    // A declaration in the init clause is scoped to the loop,
                    // so its cleanup is owed by everything inside it.
                    let inner = depth + init.iter().filter(|s| s.is_cleanup()).count();
                    self.collect_label_cleanups(std::slice::from_ref(body), inner);
                }
                Stmt::SwitchTree(switch) => {
                    self.collect_label_cleanups(std::slice::from_ref(&switch.body), depth);
                }
                Stmt::Case { body, .. } => {
                    self.collect_label_cleanups(std::slice::from_ref(body), depth);
                }
                _ => {}
            }
        }
    }

    /// Emits the cleanup calls owed by the scopes between here and `depth`,
    /// innermost first, on the edge about to be taken.
    fn leave_cleanups(&mut self, depth: usize) {
        if self.cleanups.len() <= depth || self.current.is_none() {
            return;
        }
        let owed: Vec<Expr> = self.cleanups[depth..].iter().rev().cloned().collect();
        for call in owed {
            self.push(Stmt::Expr(call));
        }
    }

    // -- statements ---------------------------------------------------------

    fn stmts(&mut self, stmts: Vec<Stmt>) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    /// Lowers a scope: its statements, then the cleanups it owes on the way
    /// out of it.
    fn scope(&mut self, stmts: Vec<Stmt>) {
        let depth = self.cleanups.len();
        self.stmts(stmts);
        self.leave_cleanups(depth);
        self.cleanups.truncate(depth);
    }

    fn stmt(&mut self, stmt: Stmt) {
        match stmt {
            Stmt::Nop => {}
            Stmt::Expr(expr) => self.push(Stmt::Expr(expr)),
            Stmt::Let {
                object,
                init,
                explicit,
            } => self.local(object, init, explicit),
            Stmt::Vla(def) => self.vla(*def),
            // The registration itself generates nothing: what it means is the
            // calls the edges out of this scope now owe.
            Stmt::Cleanup(def) => self.cleanups.push(def.call),
            Stmt::Block(items) => self.scope(items),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => self.if_stmt(cond, *then_branch, else_branch.map(|b| *b)),
            Stmt::While {
                id,
                cond,
                body,
                range,
            } => self.while_stmt(id, cond, *body, range),
            Stmt::DoWhile {
                id,
                body,
                cond,
                range,
            } => self.do_while(id, *body, cond, range),
            Stmt::For {
                id,
                init,
                cond,
                step,
                body,
                range,
            } => self.for_stmt(id, init, cond, step, *body, range),
            Stmt::SwitchTree(switch) => self.switch(*switch),
            Stmt::Case {
                switch,
                value,
                body,
                range,
            } => self.case(switch, value, *body, range),
            Stmt::Label { id, body, range } => {
                let block = self.label_block(id);
                self.jump(block, range);
                self.continue_at(block);
                self.stmt(*body);
            }
            Stmt::Goto { id, range } => {
                let block = self.label_block(id);
                // Only the scopes the jump leaves are cleaned up; the label's
                // own scopes stay, and are cleaned up where they end.
                let depth = self.label_cleanups.get(&id).copied().unwrap_or(0);
                self.leave_cleanups(depth.min(self.cleanups.len()));
                self.jump(block, range);
            }
            Stmt::GotoPtr { target, range } => self.goto_ptr(target, range),
            Stmt::Break { target, range } => {
                let block = match target {
                    BreakTarget::Loop(id) => self.loops.get(&id).map(|l| (l.brk, l.brk_cleanups)),
                    BreakTarget::Switch(id) => {
                        self.switches.get(&id).map(|s| (s.brk, s.brk_cleanups))
                    }
                };
                match block {
                    Some((block, depth)) => {
                        self.leave_cleanups(depth);
                        self.jump(block, range);
                    }
                    // Sema rejects a `break` with nothing to leave.
                    None => self.terminate(Terminator::Unreachable),
                }
            }
            Stmt::Continue { id, range } => {
                match self.loops.get(&id).map(|l| (l.cont, l.cont_cleanups)) {
                    Some((block, depth)) => {
                        self.leave_cleanups(depth);
                        self.jump(block, range);
                    }
                    None => self.terminate(Terminator::Unreachable),
                }
            }
            // The value is already in a temporary where a cleanup could see
            // it: sema puts it there, because GCC computes the result before
            // the cleanups run.
            Stmt::Return { value, range } => {
                self.leave_cleanups(0);
                self.terminate(Terminator::Return { value, range });
            }
            Stmt::Switch(_) => {
                unreachable!("sema lowers every switch into a SwitchTree in CFG mode")
            }
        }
    }

    /// GNU's computed `goto *e`.
    ///
    /// Which label it enters is a run-time value, so which scopes it leaves is
    /// one too. What is certain is that every possible target is a label whose
    /// address was taken, so the scopes *no* target is inside are left on the
    /// way — that is the shallowest of their depths, and it is exactly right
    /// in the ordinary case where every such label stands at the top of the
    /// function. Anything deeper is owed by a scope some target may still be
    /// in, and is left to the end of that scope, as a `goto` into one is.
    fn goto_ptr(&mut self, target: Expr, range: SourceRange) {
        let depth = self
            .pinned
            .iter()
            .map(|(id, _)| self.label_cleanups.get(id).copied().unwrap_or(0))
            .min()
            .unwrap_or(0);
        self.leave_cleanups(depth.min(self.cleanups.len()));
        let blocks = self.pinned.iter().map(|(_, block)| *block).collect();
        self.terminate(Terminator::IndirectJump {
            target,
            blocks,
            range,
        });
    }

    /// Hoists a local's definition and leaves its initialiser behind.
    fn local(&mut self, object: ObjectId, init: Expr, explicit: bool) {
        let info = &self.objects[object.0 as usize];
        if !matches!(info.storage, Storage::Automatic) {
            // A function-local `static` is an item of its own and keeps its
            // value across calls; it is not a local at all.
            return;
        }
        let (ty, is_const, range, name) = (info.ty, info.is_const, info.range, info.name.clone());
        let rust_name = self.unique_name(&name);
        self.locals.push(Local { object, rust_name });
        if !explicit {
            return;
        }
        let place = Place {
            kind: PlaceKind::Object(object),
            ty,
            is_const,
            range,
        };
        self.push(Stmt::Expr(Expr::new(
            ExprKind::Assign {
                place,
                value: Box::new(init),
            },
            ty,
            range,
        )));
    }

    /// Hoists the two bindings a variably modified object needs, leaving the
    /// allocation itself where the declaration was written.
    ///
    /// The storage therefore lives from the top of the function to its end
    /// rather than to the end of the block — the price of having no way to
    /// jump over a `let` — which a C program can only observe as memory it
    /// expected to have been given back. Re-reaching the declaration replaces
    /// the `Vec`, which frees the old one and is the fresh object C99 6.2.4p7
    /// asks for.
    fn vla(&mut self, def: crate::ir::VlaDef) {
        for object in [def.storage, def.object] {
            let name = self.objects[object.0 as usize].name.clone();
            let rust_name = self.unique_name(&name);
            self.locals.push(Local { object, rust_name });
        }
        self.push(Stmt::Vla(Box::new(def)));
    }

    /// A Rust name for a hoisted local, derived from the C one.
    fn unique_name(&mut self, name: &str) -> String {
        if self.used.insert(name.to_owned()) {
            return name.to_owned();
        }
        for n in 1u32.. {
            let candidate = format!("{name}_{n}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
        }
        unreachable!("the loop above always terminates")
    }

    fn if_stmt(&mut self, cond: Expr, then_branch: Stmt, else_branch: Option<Stmt>) {
        let range = cond.range;
        let then_blk = self.new_block();
        let else_blk = self.new_block();
        let join = if else_branch.is_some() {
            self.new_block()
        } else {
            else_blk
        };
        self.terminate(Terminator::Branch {
            cond,
            then_blk,
            else_blk,
        });
        self.continue_at(then_blk);
        self.stmt(then_branch);
        self.jump(join, range);
        if let Some(else_branch) = else_branch {
            self.continue_at(else_blk);
            self.stmt(else_branch);
            self.jump(join, range);
        }
        self.continue_at(join);
    }

    fn while_stmt(&mut self, id: LoopId, cond: Expr, body: Stmt, range: SourceRange) {
        let head = self.new_block();
        let body_blk = self.new_block();
        let exit = self.new_block();
        self.jump(head, range);
        self.continue_at(head);
        self.test(cond, body_blk, exit, range);
        let depth = self.cleanups.len();
        self.loops.insert(
            id,
            LoopBlocks {
                brk: exit,
                brk_cleanups: depth,
                cont: head,
                cont_cleanups: depth,
            },
        );
        self.continue_at(body_blk);
        self.stmt(body);
        self.jump(head, range);
        self.continue_at(exit);
    }

    fn do_while(&mut self, id: LoopId, body: Stmt, cond: Expr, range: SourceRange) {
        let body_blk = self.new_block();
        let test = self.new_block();
        let exit = self.new_block();
        self.jump(body_blk, range);
        let depth = self.cleanups.len();
        self.loops.insert(
            id,
            LoopBlocks {
                brk: exit,
                brk_cleanups: depth,
                cont: test,
                cont_cleanups: depth,
            },
        );
        self.continue_at(body_blk);
        self.stmt(body);
        self.jump(test, range);
        self.continue_at(test);
        self.test(cond, body_blk, exit, range);
        self.continue_at(exit);
    }

    fn for_stmt(
        &mut self,
        id: LoopId,
        init: Vec<Stmt>,
        cond: Option<Expr>,
        step: Option<Expr>,
        body: Stmt,
        range: SourceRange,
    ) {
        // C99 scopes a declaration in the init clause to the whole loop, so a
        // `cleanup` on one is owed by everything that leaves the loop — the
        // `break` and the falling-out edge, but not the `continue`, which
        // stays inside.
        let outer = self.cleanups.len();
        self.stmts(init);
        let inner = self.cleanups.len();
        let head = self.new_block();
        let body_blk = self.new_block();
        let step_blk = self.new_block();
        let exit = self.new_block();
        self.jump(head, range);
        self.continue_at(head);
        match cond {
            Some(cond) => self.test(cond, body_blk, exit, range),
            None => self.jump(body_blk, range),
        }
        self.loops.insert(
            id,
            LoopBlocks {
                // The exit block runs what the init clause owes, whichever
                // way the loop was left, so a `break` only has the body's.
                brk: exit,
                brk_cleanups: inner,
                cont: step_blk,
                cont_cleanups: inner,
            },
        );
        self.continue_at(body_blk);
        self.stmt(body);
        self.jump(step_blk, range);
        self.continue_at(step_blk);
        if let Some(step) = step {
            self.push(Stmt::Expr(step));
        }
        self.jump(head, range);
        self.continue_at(exit);
        // Both ways out of the loop arrive here; the cleanups the init clause
        // owes run once, on the way past.
        self.leave_cleanups(outer);
        self.cleanups.truncate(outer);
    }

    /// Ends the current block on a loop's controlling expression, which a
    /// constantly true one turns into an unconditional edge.
    fn test(&mut self, cond: Expr, then_blk: BlockId, else_blk: BlockId, range: SourceRange) {
        if is_always_true(&cond) {
            self.jump(then_blk, range);
            return;
        }
        self.terminate(Terminator::Branch {
            cond,
            then_blk,
            else_blk,
        });
    }

    fn switch(&mut self, switch: crate::ir::SwitchTree) {
        let crate::ir::SwitchTree {
            id,
            scrutinee,
            body,
            range,
        } = switch;
        let dispatch = self.current();
        let exit = self.new_block();
        self.switches.insert(
            id,
            SwitchFrame {
                brk: exit,
                brk_cleanups: self.cleanups.len(),
                cases: Vec::new(),
                default: None,
            },
        );
        // Control enters the body at a label, never at its first statement, so
        // the body starts a block of its own — one that is unreachable unless
        // something jumps into it.
        self.current = None;
        self.stmt(*body);
        self.jump(exit, range);
        let frame = self
            .switches
            .remove(&id)
            .expect("the frame was just inserted");
        self.blocks[dispatch.index()].term = Terminator::Switch {
            value: scrutinee,
            cases: frame.cases,
            default: frame.default.unwrap_or(exit),
            range,
        };
        self.continue_at(exit);
    }

    fn case(&mut self, switch: SwitchId, value: Option<CaseRange>, body: Stmt, range: SourceRange) {
        let block = self.new_block();
        // A group falls through into the next one, so the statements before
        // the label flow here too.
        self.jump(block, range);
        self.continue_at(block);
        if let Some(frame) = self.switches.get_mut(&switch) {
            match value {
                Some(value) => frame.cases.push((value, block)),
                None => frame.default = Some(block),
            }
        }
        self.stmt(body);
    }

    // -- cleanup ------------------------------------------------------------

    fn finish(mut self, entry: BlockId) -> Cfg {
        let entry = self.thread_jumps(entry);
        self.merge_chains(entry);
        self.renumber(entry)
    }

    /// Whether a block's identity has to survive the clean-up passes.
    ///
    /// A block a `&&label` named has a *number* the program computed, so it
    /// may not be threaded past, merged into a predecessor or dropped for
    /// being unreachable — nothing else reaches it, and a computed `goto`
    /// still can.
    fn is_pinned(&self, block: BlockId) -> bool {
        self.pinned.iter().any(|(_, pinned)| *pinned == block)
    }

    /// Removes blocks that only jump somewhere else, re-pointing every edge.
    fn thread_jumps(&mut self, entry: BlockId) -> BlockId {
        let resolved: Vec<BlockId> = (0..self.blocks.len())
            .map(|index| self.resolve(BlockId(index as u32)))
            .collect();
        for block in &mut self.blocks {
            block.term.map_targets(|target| resolved[target.index()]);
        }
        resolved[entry.index()]
    }

    /// Follows a chain of statement-free jumps to the block that really runs.
    fn resolve(&self, mut block: BlockId) -> BlockId {
        let mut seen = HashSet::new();
        while seen.insert(block) {
            let candidate = &self.blocks[block.index()];
            if !candidate.stmts.is_empty() || self.is_pinned(block) {
                break;
            }
            match candidate.term {
                Terminator::Jump { target, .. } if target != block => block = target,
                _ => break,
            }
        }
        block
    }

    /// Appends a block to its predecessor when that is the only way in.
    fn merge_chains(&mut self, entry: BlockId) {
        let reachable = self.reachable(&self.roots(entry));
        let mut predecessors = vec![0usize; self.blocks.len()];
        for id in &reachable {
            for successor in self.blocks[id.index()].term.successors() {
                predecessors[successor.index()] += 1;
            }
        }
        for id in &reachable {
            while let Terminator::Jump { target, .. } = self.blocks[id.index()].term {
                if target == *id
                    || target == entry
                    || predecessors[target.index()] != 1
                    || self.is_pinned(target)
                {
                    break;
                }
                let mut stmts = std::mem::take(&mut self.blocks[target.index()].stmts);
                let term = std::mem::replace(
                    &mut self.blocks[target.index()].term,
                    Terminator::Unreachable,
                );
                self.blocks[id.index()].stmts.append(&mut stmts);
                self.blocks[id.index()].term = term;
                predecessors[target.index()] = 0;
            }
        }
    }

    /// Where the walks start: the entry, and then every pinned block.
    ///
    /// A pinned block may be reachable through no edge at all — nothing but a
    /// computed `goto` enters it, and a function may take a label's address
    /// without ever jumping through it — so it is a root of its own. They come
    /// after the entry so that the numbering still reads top to bottom for
    /// everything the entry reaches.
    fn roots(&self, entry: BlockId) -> Vec<BlockId> {
        let mut roots = vec![entry];
        roots.extend(self.pinned.iter().map(|(_, block)| *block));
        roots
    }

    /// The blocks control can reach from `roots`.
    fn reachable(&self, roots: &[BlockId]) -> Vec<BlockId> {
        let mut seen = HashSet::new();
        let mut stack = roots.to_vec();
        let mut out = Vec::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            out.push(id);
            stack.extend(self.blocks[id.index()].term.successors());
        }
        out.sort_unstable();
        out
    }

    /// Numbers the reachable blocks in reverse postorder and drops the rest.
    ///
    /// Reverse postorder is what makes the emitted states read in the order
    /// the C did: a block comes before everything only reachable through it,
    /// and the `then` branch of a condition comes before the `else`.
    fn renumber(mut self, entry: BlockId) -> Cfg {
        let mut order = Vec::with_capacity(self.blocks.len());
        let mut visited = vec![false; self.blocks.len()];
        for root in self.roots(entry) {
            if visited[root.index()] {
                continue;
            }
            let mut postorder = Vec::new();
            let mut stack = vec![(root, 0usize)];
            visited[root.index()] = true;
            while let Some((id, next)) = stack.pop() {
                let successors = self.blocks[id.index()].term.successors();
                // Successors are pushed in reverse so that the first one is
                // explored last and therefore ends up first once the postorder
                // is reversed.
                if next < successors.len() {
                    stack.push((id, next + 1));
                    let successor = successors[successors.len() - 1 - next];
                    if !visited[successor.index()] {
                        visited[successor.index()] = true;
                        stack.push((successor, 0));
                    }
                    continue;
                }
                postorder.push(id);
            }
            postorder.reverse();
            order.extend(postorder);
        }

        let mut index_of = vec![None; self.blocks.len()];
        for (index, id) in order.iter().enumerate() {
            index_of[id.index()] = Some(BlockId(index as u32));
        }
        let mut blocks = Vec::with_capacity(order.len());
        for id in order {
            let mut block = std::mem::replace(
                &mut self.blocks[id.index()],
                BasicBlock {
                    stmts: Vec::new(),
                    term: Terminator::Unreachable,
                },
            );
            block.term.map_targets(|target| {
                index_of[target.index()].expect("a reachable block only names reachable blocks")
            });
            blocks.push(block);
        }
        let labels = self
            .pinned
            .iter()
            .map(|(id, block)| {
                let numbered = index_of[block.index()]
                    .expect("a pinned block is a root of the walk and is always numbered");
                (*id, numbered)
            })
            .collect();
        Cfg {
            locals: self.locals,
            blocks,
            labels,
        }
    }
}
