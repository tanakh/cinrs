//! Recovering Rust's own control flow from the [graph](crate::cfg).
//!
//! The graph is a correct lowering of any `goto`, and as a flat
//! `loop { match state { … } }` it is also an opaque one: every edge is a store
//! and a jump back to a dispatch, and the loops the C program had are gone —
//! which is what made SQLite's `sqlite3VdbeExec` run five times the
//! instructions GCC's does. This module puts them back. It reads the finished
//! graph and rebuilds a *structured* program out of three shapes, which
//! [`codegen`](crate::codegen) emits as ordinary Rust:
//!
//! * **Simple** — one basic block. Its terminator becomes an `if` or a `match`,
//!   and the blocks that only *it* can reach are emitted inside the arms, so a
//!   two-way branch is `if c { … } else { … }` and a `switch` is a `match` with
//!   the case bodies in it.
//! * **Loop** — a set of blocks with an entry that something inside jumps back
//!   to. It becomes `'l: loop { … }`: a back edge is `continue 'l` and an exit
//!   is `break 'l`. The label is the C label the head carries, where there is
//!   one.
//! * **Sequence** — the run of shapes at one level, in an order in which every
//!   remaining jump goes *forward*. A forward jump is `break` of a labelled
//!   block that ends where its target begins, which is the same trick
//!   [`regions`](crate::regions) plays on the statement tree, and the reason
//!   nothing here needs a variable to say where control went.
//!
//! The algorithm is Emscripten's relooper — Alon Zakai, *Emscripten: An
//! LLVM-to-JavaScript Compiler*, OOPSLA 2011, §3.2 — as it is also implemented
//! in `c2rust-transpile`'s `cfg::relooper`. It was written from the description
//! of the algorithm rather than from either implementation.
//!
//! # The recursion
//!
//! `process(entries, blocks)` builds the shapes for `blocks`, which control can
//! only get into at one of `entries`:
//!
//! 1. **One entry that nothing in `blocks` jumps back to** → a Simple for it,
//!    then `process` of what its terminator reaches.
//! 2. **Several entries** → try a Multiple: give each entry the blocks that
//!    *only it* can reach, lay those groups out one after another, and
//!    `process` the rest after them. An entry another entry can reach owns
//!    nothing and waits for the rest.
//! 3. **Otherwise** → a Loop. Its body is the entries plus every block that can
//!    get back to one of them; what is left follows it. The back edges are then
//!    *taken out of the graph*, which is what lets the body be relooped as if
//!    its entries were entered from outside only.
//!
//! # Irreducible regions
//!
//! A `goto` into the middle of a loop, Duff's device and two loops that jump
//! into each other's bodies all produce a cycle with two heads, which no
//! arrangement of Rust's blocks and loops can express. Step 3 is where they
//! land: the Loop keeps *both* heads, and once its back edges are cut, step 2
//! splits the body into one group per head. That dispatch — and only that one —
//! reads a state variable, which every jump to a head writes first. It is one
//! `u32` per irreducible region rather than one per function, and the blocks
//! outside the region never touch it.
//!
//! # What the shapes preserve
//!
//! Everything, because the graph already holds it. A `cleanup` call and a
//! variable length array's release are *statements at the end of the block the
//! edge leaves from* — [`cfg`](crate::cfg) puts them there — so no shape has to
//! know about them. `switch` fallthrough is an edge from one case block to the
//! next, and comes out as the case body falling out of its `match` arm into the
//! code after the `match`. A `return` is a block terminator wherever it stands.
//!
//! # Giving up
//!
//! Three things keep the [state machine](crate::cfg) alive. GNU's `&&label` is a
//! *state number*, so a function that takes a label's address is lowered the
//! old way, whole. A `switch` whose `case`s fall through one into the next gives
//! each of them a labelled block, and past two hundred of those `rustc`'s own
//! parser runs out of stack, where the machine's flat `match` does not. And
//! the shapes are checked before they are handed over: if every block is not in
//! the tree exactly once, or if some jump has nothing to break to, [`plan`]
//! answers `None` and the state machine runs instead of something subtly wrong.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::cfg::{BasicBlock, BlockId, Terminator};

/// A set of blocks, in block order.
type Set = BTreeSet<BlockId>;

/// Where control arrives when it runs off the end of a shape, or when it
/// `break`s or `continue`s a labelled one.
///
/// More than one target means an irreducible region's dispatch: which of them
/// is entered is the value written to [`Exit::state`] on the way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exit {
    /// The blocks control can arrive at.
    pub targets: Vec<BlockId>,
    /// The state variable that says which, when there is more than one.
    pub state: Option<u32>,
}

impl Exit {
    /// Nothing follows: control cannot leave this way.
    pub fn nowhere() -> Self {
        Self::default()
    }

    /// One block follows, with no dispatch in between.
    pub fn to(target: BlockId) -> Self {
        Self {
            targets: vec![target],
            state: None,
        }
    }

    /// Which entry `target` is, if this exit reaches it.
    pub fn index_of(&self, target: BlockId) -> Option<usize> {
        self.targets.iter().position(|block| *block == target)
    }
}

/// A run of shapes, one after another.
///
/// A shape may jump to the entry of any shape *after* it in the run, which is a
/// `break` of a labelled block that ends where that shape begins; the last one
/// falls out of the run.
pub type Seq = Vec<Shape>;

/// One entry of a branch, and the shapes that run it.
#[derive(Clone, Debug)]
pub struct Arm {
    /// The block the branch enters.
    pub entry: BlockId,
    /// What runs there, up to where the arms meet again.
    pub body: Seq,
}

/// A piece of recovered control flow.
#[derive(Clone, Debug)]
pub enum Shape {
    /// One basic block: its statements, then its terminator.
    Simple {
        /// The block.
        block: BlockId,
        /// The targets of its terminator that nothing else reaches, emitted
        /// inside the `if` or the `match` the terminator becomes. Every other
        /// target becomes a jump.
        arms: Vec<Arm>,
    },
    /// `'l: loop { … }`.
    Loop {
        /// Distinguishes this loop's label from the others in the function.
        id: u32,
        /// The C label its head carries, which the Rust label is named after.
        name: Option<String>,
        /// The blocks it is entered at: one for a natural loop, several for an
        /// irreducible region.
        entries: Vec<BlockId>,
        /// The state variable that picks the entry, for such a region.
        state: Option<u32>,
        /// What runs inside. For an irreducible region the first shape is the
        /// [dispatch](Shape::Dispatch) over `state`.
        body: Seq,
    },
    /// The head of an irreducible region: a `match` on the state variable that
    /// picks which entry this turn round the loop runs.
    Dispatch {
        /// The state variable it reads.
        state: u32,
        /// One arm per entry, in the order the state numbers them.
        arms: Vec<Arm>,
    },
}

impl Shape {
    /// Where a jump to this shape arrives.
    pub fn exit(&self) -> Exit {
        match self {
            Shape::Simple { block, .. } => Exit::to(*block),
            Shape::Loop { entries, state, .. } => Exit {
                targets: entries.clone(),
                state: *state,
            },
            Shape::Dispatch { state, arms } => Exit {
                targets: arms.iter().map(|arm| arm.entry).collect(),
                state: Some(*state),
            },
        }
    }
}

/// A whole function body, recovered.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The shapes of the body.
    pub body: Seq,
    /// How many state variables the irreducible regions need; they are
    /// numbered from zero.
    pub states: u32,
}

/// Recovers the structured form of a graph, or answers `None`.
///
/// `names` is the C label each block stands at, where it stands at one, which
/// is what the loops are named after. `None` means the [state
/// machine](crate::cfg) has to be used: a function whose labels have addresses,
/// or — which has not been observed, and is checked for rather than trusted —
/// one whose shapes would not account for every block or would leave a jump
/// with nothing to break to.
pub fn plan(blocks: &[BasicBlock], names: &HashMap<BlockId, String>) -> Option<Plan> {
    if blocks.is_empty() {
        return None;
    }
    // A computed `goto` jumps to a number, and the number is the state a block
    // was given; there is no structured form of that.
    if blocks
        .iter()
        .any(|block| matches!(block.term, Terminator::IndirectJump { .. }))
    {
        return None;
    }
    let mut relooper = Relooper::new(blocks, names);
    let all: Set = (0..blocks.len() as u32).map(BlockId).collect();
    let entries: Set = [BlockId(0)].into_iter().collect();
    let body = relooper.process(&entries, &all);
    let plan = Plan {
        body,
        states: relooper.states,
    };
    let mut seen = vec![0usize; blocks.len()];
    count_blocks(&plan.body, &mut seen);
    if seen.iter().any(|times| *times != 1) {
        return None;
    }
    if nesting(&plan.body, 0) > MAX_NESTING {
        return None;
    }
    let mut scopes = Vec::new();
    resolves(blocks, &plan.body, &Exit::nowhere(), &mut scopes).then_some(plan)
}

/// How deeply the output may nest blocks and loops.
///
/// A `switch` whose `case`s fall through one into the next gives each of them a
/// labelled block of its own, and `rustc`'s own parser runs out of stack
/// somewhere past four hundred of those — which is what the same figure in
/// [`sema`](crate::sema) is for. Past this, the [state machine](crate::cfg),
/// whose `match` is flat however many arms it has, is the answer.
const MAX_NESTING: usize = 200;

/// How deeply the shapes of `seq` nest, given that they already stand `depth`
/// blocks deep.
///
/// It stops counting past [`MAX_NESTING`], which is also what bounds its own
/// recursion — and the recursion of everything that walks a plan that passed
/// the check, [`codegen`](crate::codegen) included.
fn nesting(seq: &Seq, depth: usize) -> usize {
    if depth > MAX_NESTING {
        return depth;
    }
    let last = seq.len().saturating_sub(1);
    let mut out = depth;
    for (index, shape) in seq.iter().enumerate() {
        // Everything but the last shape stands inside one labelled block per
        // shape that follows it.
        let here = depth + (last - index);
        out = out.max(match shape {
            Shape::Simple { arms, .. } | Shape::Dispatch { arms, .. } => arms
                .iter()
                .map(|arm| nesting(&arm.body, here + 1))
                .max()
                .unwrap_or(here),
            Shape::Loop { body, .. } => nesting(body, here + 1),
        });
        if out > MAX_NESTING {
            return out;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// the recursion
// ---------------------------------------------------------------------------

struct Relooper<'a> {
    blocks: &'a [BasicBlock],
    names: &'a HashMap<BlockId, String>,
    succ: Vec<Vec<BlockId>>,
    preds: Vec<Vec<BlockId>>,
    /// The back edges a loop has taken out of the graph.
    ///
    /// A jump from inside a loop to its head is a `continue`, not an edge the
    /// shapes inside it have to follow; hiding it is what lets the body be
    /// relooped as though its heads were entered from outside only, and is
    /// what turns an irreducible region into a dispatch over its heads.
    cut: HashSet<(BlockId, BlockId)>,
    states: u32,
    loops: u32,
}

impl<'a> Relooper<'a> {
    fn new(blocks: &'a [BasicBlock], names: &'a HashMap<BlockId, String>) -> Self {
        let succ: Vec<Vec<BlockId>> = blocks
            .iter()
            .map(|block| {
                let mut out = block.term.successors();
                out.sort_unstable();
                out.dedup();
                out
            })
            .collect();
        let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); blocks.len()];
        for (index, targets) in succ.iter().enumerate() {
            for target in targets {
                preds[target.0 as usize].push(BlockId(index as u32));
            }
        }
        Self {
            blocks,
            names,
            succ,
            preds,
            cut: HashSet::new(),
            states: 0,
            loops: 0,
        }
    }

    /// The successors of `block` that are still in `set` and whose edge is
    /// still in the graph.
    fn succ_in(&self, block: BlockId, set: &Set) -> Vec<BlockId> {
        self.succ[block.0 as usize]
            .iter()
            .copied()
            .filter(|target| set.contains(target) && !self.cut.contains(&(block, *target)))
            .collect()
    }

    /// Whether anything still in `set` jumps to `block`.
    fn has_pred_in(&self, block: BlockId, set: &Set) -> bool {
        self.preds[block.0 as usize]
            .iter()
            .any(|from| set.contains(from) && !self.cut.contains(&(*from, block)))
    }

    /// The shapes of `blocks`, which control enters at one of `entries`.
    ///
    /// What each shape leaves behind is the next turn of the loop below rather
    /// than a recursive call: a run of straight-line blocks is one Simple after
    /// another, and a function with a thousand of them would otherwise be a
    /// thousand frames deep, each holding a copy of the block set.
    fn process(&mut self, entries: &Set, blocks: &Set) -> Seq {
        let mut out = Seq::new();
        let mut entries = entries.clone();
        let mut blocks = blocks.clone();
        loop {
            entries.retain(|block| blocks.contains(block));
            if entries.is_empty() {
                return out;
            }
            entries = if entries.len() == 1 {
                let entry = *entries.iter().next().expect("one entry");
                if self.has_pred_in(entry, &blocks) {
                    self.make_loop(&entries, &mut blocks, &mut out)
                } else {
                    self.make_simple(entry, &mut blocks, &mut out)
                }
            } else {
                // Several entries: they may be the arms of a branch, which is a
                // Multiple, or the heads of one tangle, which is a Loop. The
                // Multiple is tried first — an `if` whose body is a `while` has
                // two entries and a back edge to one of them, and is an `if`
                // around a loop rather than a loop with a dispatch at its head.
                match self.make_multiple(&entries, &mut blocks, &mut out) {
                    Some(next) => next,
                    None => self.make_loop(&entries, &mut blocks, &mut out),
                }
            };
        }
    }

    /// One block, and what its terminator reaches.
    fn make_simple(&mut self, entry: BlockId, blocks: &mut Set, out: &mut Seq) -> Set {
        blocks.remove(&entry);
        let targets: Set = self.succ_in(entry, blocks).into_iter().collect();
        // The blocks only this terminator can reach go inside the `if` or the
        // `match` it becomes, which is what keeps a branch a branch.
        if self.fusable(entry, &targets)
            && let Some((arms, next)) = self.groups(&targets, blocks)
        {
            out.push(Shape::Simple { block: entry, arms });
            return next;
        }
        out.push(Shape::Simple {
            block: entry,
            arms: Vec::new(),
        });
        targets
    }

    /// Whether the arms of `entry`'s terminator may hold the code of the blocks
    /// they enter.
    ///
    /// They may not when one block is the target of two of them — a `switch`
    /// whose `default` is also a `case`, say — because the code would then be
    /// generated twice.
    fn fusable(&self, entry: BlockId, targets: &Set) -> bool {
        if targets.len() < 2 {
            return false;
        }
        let listed = self.terminator_targets(entry);
        targets
            .iter()
            .all(|target| listed.iter().filter(|other| *other == target).count() <= 1)
    }

    /// The blocks a terminator names, with the repeats a `match` would have to
    /// generate twice.
    fn terminator_targets(&self, block: BlockId) -> Vec<BlockId> {
        match &self.blocks[block.0 as usize].term {
            Terminator::Switch { cases, default, .. } => {
                let mut out: Vec<BlockId> = Vec::new();
                for (_, target) in cases {
                    if !out.contains(target) {
                        // Two `case` labels on one block share an arm.
                        out.push(*target);
                    }
                }
                out.push(*default);
                out
            }
            other => other.successors(),
        }
    }

    /// Lays the entries out one after another, each with the blocks only it can
    /// reach.
    ///
    /// A group jumps forwards to whatever follows the last of them, which is a
    /// `break` of the labelled block it stands in — so laying them out flat,
    /// rather than nesting each one, is all a Multiple is here.
    fn make_multiple(&mut self, entries: &Set, blocks: &mut Set, out: &mut Seq) -> Option<Set> {
        let (arms, next) = self.groups(entries, blocks)?;
        for arm in arms {
            out.extend(arm.body);
        }
        Some(next)
    }

    /// The groups of a Multiple and the entries left for what follows them, or
    /// `None` when every entry is reachable from another and nothing can be
    /// split off. `blocks` is left holding what the groups did not take.
    fn groups(&mut self, entries: &Set, blocks: &mut Set) -> Option<(Vec<Arm>, Set)> {
        let owned = self.independent_groups(entries, blocks);
        if owned.is_empty() {
            return None;
        }
        let mut consumed = Set::new();
        for (_, group) in &owned {
            consumed.extend(group.iter().copied());
        }
        for block in &consumed {
            blocks.remove(block);
        }
        let mut next: Set = entries
            .iter()
            .copied()
            .filter(|entry| !consumed.contains(entry))
            .collect();
        for block in &consumed {
            next.extend(self.succ_in(*block, blocks));
        }
        let arms: Vec<Arm> = owned
            .into_iter()
            .map(|(entry, group)| {
                let one: Set = [entry].into_iter().collect();
                Arm {
                    entry,
                    body: self.process(&one, &group),
                }
            })
            .collect();
        Some((arms, next))
    }

    /// For each entry, the blocks *only* that entry can reach.
    ///
    /// Such a group is closed: every jump into it from inside `blocks` comes
    /// from the group itself or from its entry, because anything else would be
    /// a second entry the block is reachable from. That is what lets the group
    /// be emitted on its own — inside a `match` arm, or as one run of a
    /// sequence. An entry another entry can reach owns nothing at all and is
    /// left for the shapes that follow.
    fn independent_groups(&self, entries: &Set, blocks: &Set) -> Vec<(BlockId, Set)> {
        let count = self.blocks.len();
        let mut owner: Vec<Option<BlockId>> = vec![None; count];
        let mut shared = vec![false; count];
        for entry in entries {
            owner[entry.0 as usize] = Some(*entry);
        }
        let mut seen = vec![false; count];
        let mut stack: Vec<BlockId> = Vec::new();
        for entry in entries {
            seen.iter_mut().for_each(|flag| *flag = false);
            stack.clear();
            stack.extend(self.succ_in(*entry, blocks));
            for target in &stack {
                seen[target.0 as usize] = true;
            }
            while let Some(block) = stack.pop() {
                match owner[block.0 as usize] {
                    None => owner[block.0 as usize] = Some(*entry),
                    Some(other) if other != *entry => shared[block.0 as usize] = true,
                    Some(_) => {}
                }
                for target in self.succ_in(block, blocks) {
                    if !seen[target.0 as usize] {
                        seen[target.0 as usize] = true;
                        stack.push(target);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for entry in entries {
            if shared[entry.0 as usize] {
                continue;
            }
            let group: Set = blocks
                .iter()
                .copied()
                .filter(|block| {
                    owner[block.0 as usize] == Some(*entry) && !shared[block.0 as usize]
                })
                .collect();
            out.push((*entry, group));
        }
        out
    }

    /// A loop over the entries and everything that can get back to one of them.
    fn make_loop(&mut self, entries: &Set, blocks: &mut Set, out: &mut Seq) -> Set {
        let mut inner = entries.clone();
        inner.extend(self.reaching(entries, blocks));
        // One way out needs no labelled block around the loop, so there is
        // nothing to gain by widening it; more than one is where it pays.
        if self.leaving(blocks, &inner).len() > 1 {
            self.widen_loop(blocks, &mut inner);
        }
        let follow = self.leaving(blocks, &inner);
        for block in &inner {
            blocks.remove(block);
        }
        // Take the back edges out: inside the body they are `continue`s.
        for entry in entries {
            for from in self.preds[entry.0 as usize].clone() {
                if inner.contains(&from) {
                    self.cut.insert((from, *entry));
                }
            }
        }
        let id = self.loops;
        self.loops += 1;
        let name = entries
            .iter()
            .find_map(|entry| self.names.get(entry))
            .cloned();
        let (state, entry_list, body) = if entries.len() == 1 {
            let body = self.process(entries, &inner);
            (None, entries.iter().copied().collect(), body)
        } else {
            // Every head now stands on its own — nothing inside jumps to one
            // any more — so the body splits into one group per head, and the
            // state variable is what says which one this turn runs.
            let state = self.states;
            self.states += 1;
            let mut rest = inner.clone();
            match self.groups(entries, &mut rest) {
                Some((arms, next)) => {
                    let list: Vec<BlockId> = arms.iter().map(|arm| arm.entry).collect();
                    let mut body = vec![Shape::Dispatch { state, arms }];
                    body.extend(self.process(&next, &rest));
                    (Some(state), list, body)
                }
                // Cannot happen: cutting the back edges leaves every head
                // without a predecessor inside. The check in `plan` turns it
                // into the state machine rather than into wrong code.
                None => (Some(state), entries.iter().copied().collect(), Seq::new()),
            }
        };
        out.push(Shape::Loop {
            id,
            name,
            entries: entry_list,
            state,
            body,
        });
        follow
    }

    /// Pulls the runs that leave the loop back into its body.
    ///
    /// Only the entries and what can get back to them *have* to be inside: a
    /// `case` that ends in `goto fail` cannot reach the head, so the rule above
    /// leaves it after the loop, and then everything before it has to stand
    /// inside a labelled block so that the dispatch can break to it. A block
    /// only one thing jumps to is not a place anything has to break to, so
    /// following those chains in costs nothing and is what keeps the body of a
    /// `switch` inside the `switch` — the difference between a `match` arm and
    /// a labelled block per `case`. (`c2rust` calls this `heuristic_loop_body`;
    /// the idea is the same.)
    fn widen_loop(&self, blocks: &Set, inner: &mut Set) {
        let mut queue: Vec<BlockId> = self.leaving(blocks, inner).into_iter().collect();
        while let Some(mut block) = queue.pop() {
            loop {
                if inner.contains(&block) {
                    break;
                }
                let entered = self.preds[block.0 as usize]
                    .iter()
                    .filter(|from| blocks.contains(from) && !self.cut.contains(&(**from, block)))
                    .count();
                if entered != 1 {
                    break;
                }
                inner.insert(block);
                let targets = self.succ_in(block, blocks);
                let Some((next, rest)) = targets.split_first() else {
                    break;
                };
                queue.extend(rest.iter().copied());
                block = *next;
            }
        }
    }

    /// The blocks of `blocks` that `part` jumps to from the outside of `part`.
    fn leaving(&self, blocks: &Set, part: &Set) -> Set {
        let mut out = Set::new();
        for block in part {
            for target in self.succ_in(*block, blocks) {
                if !part.contains(&target) {
                    out.insert(target);
                }
            }
        }
        out
    }

    /// The blocks of `set` that can get to one of `targets` without leaving it.
    fn reaching(&self, targets: &Set, set: &Set) -> Set {
        let mut seen = vec![false; self.blocks.len()];
        let mut out = Set::new();
        let mut stack: Vec<BlockId> = targets.iter().copied().collect();
        let mut todo: Vec<BlockId> = Vec::new();
        while let Some(block) = stack.pop() {
            for from in &self.preds[block.0 as usize] {
                if !set.contains(from)
                    || self.cut.contains(&(*from, block))
                    || seen[from.0 as usize]
                {
                    continue;
                }
                seen[from.0 as usize] = true;
                out.insert(*from);
                todo.push(*from);
            }
            stack.append(&mut todo);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// the checks
// ---------------------------------------------------------------------------

/// How many times each block stands in the tree.
fn count_blocks(seq: &Seq, out: &mut [usize]) {
    for shape in seq {
        match shape {
            Shape::Simple { block, arms } => {
                out[block.0 as usize] += 1;
                for arm in arms {
                    count_blocks(&arm.body, out);
                }
            }
            Shape::Loop { body, .. } => count_blocks(body, out),
            Shape::Dispatch { arms, .. } => {
                for arm in arms {
                    count_blocks(&arm.body, out);
                }
            }
        }
    }
}

/// Whether every jump the tree still holds has somewhere to go.
///
/// It walks exactly as [`codegen`](crate::codegen) emits: the shapes of a
/// sequence in order, with a labelled block open around everything before each
/// of them, and a loop's head and follow reachable from inside it. A `false`
/// would be a bug in this module, and answering it is what keeps such a bug
/// from becoming generated code that jumps to the wrong place.
fn resolves(blocks: &[BasicBlock], seq: &Seq, fall: &Exit, scopes: &mut Vec<Exit>) -> bool {
    let exits: Vec<Exit> = seq.iter().map(Shape::exit).collect();
    let depth = scopes.len();
    for exit in exits.iter().skip(1).rev() {
        scopes.push(exit.clone());
    }
    let mut ok = true;
    for (index, shape) in seq.iter().enumerate() {
        if index > 0 {
            scopes.pop();
        }
        let next = exits.get(index + 1).unwrap_or(fall);
        ok &= resolves_shape(blocks, shape, next, scopes);
    }
    scopes.truncate(depth);
    ok
}

fn resolves_shape(
    blocks: &[BasicBlock],
    shape: &Shape,
    fall: &Exit,
    scopes: &mut Vec<Exit>,
) -> bool {
    match shape {
        Shape::Simple { block, arms } => {
            let mut ok = true;
            for target in blocks[block.0 as usize].term.successors() {
                match arms.iter().find(|arm| arm.entry == target) {
                    Some(arm) => ok &= resolves(blocks, &arm.body, fall, scopes),
                    None => {
                        ok &= fall.index_of(target).is_some()
                            || scopes.iter().any(|exit| exit.index_of(target).is_some());
                    }
                }
            }
            ok
        }
        Shape::Loop {
            entries,
            state,
            body,
            ..
        } => {
            let head = Exit {
                targets: entries.clone(),
                state: *state,
            };
            scopes.push(head.clone());
            scopes.push(fall.clone());
            let ok = resolves(blocks, body, &head, scopes);
            scopes.pop();
            scopes.pop();
            ok
        }
        Shape::Dispatch { arms, .. } => arms
            .iter()
            .all(|arm| resolves(blocks, &arm.body, fall, scopes)),
    }
}
