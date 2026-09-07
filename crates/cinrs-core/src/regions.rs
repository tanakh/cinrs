//! The `goto`s Rust can express on its own, and the regions they become.
//!
//! Most `goto`s in real C go *outwards*: `goto done` and `goto fail` leave a
//! nest of loops for a cleanup label near the end of the function, and
//! `goto retry` restarts a run of statements the label stands at the head of.
//! Neither needs the [state machine](crate::cfg) — Rust has a labelled block
//! and a labelled loop, and this module is what recognises the shapes that fit
//! in them. What is left over, and only that, goes through the CFG.
//!
//! # The two shapes
//!
//! Both are stated in terms of a **statement list** — the items of a compound
//! statement — and a label that is a direct child of it:
//!
//! ```text
//! s0; s1; goto done; s2;      →   'done: { s0; s1; break 'done; s2; }
//! done: s3;                       s3;
//!
//! retry: s0; goto retry; s1;  →   'retry: loop { s0; continue 'retry; s1; break 'retry; }
//! s2;                             s2;
//! ```
//!
//! A **block** ends exactly where the label stands, because that is where a
//! `break` out of it continues; where it *starts* is free, so it starts at the
//! first `goto` that names the label rather than at the head of the list. A
//! **loop** starts exactly where the label stands, because that is where a
//! `continue` re-enters; where it *ends* is free, so it ends just after the
//! last `goto`. A label with jumps of both kinds gets both, and they are
//! adjacent rather than nested — a forward `goto` breaks the block and falls
//! straight into the loop the label heads.
//!
//! `break` and `continue` inside a region still mean what C said: every loop
//! and every `switch` in the generated code carries a label of its own, so
//! nothing here can capture one. A `goto` out of a `switch` is a `break` past
//! the labelled-block chain the `switch` became, which is exactly what C asks
//! for.
//!
//! # How they nest
//!
//! The free end is what makes them nest. The regions of one list have to form
//! a tree, so the walk takes the outermost of them — the *last* label a
//! forward jump reaches, whose block ends after every other block in the list,
//! or failing that the *first* label a backward jump returns to, whose loop
//! starts before every other loop — widens its free end until every other
//! region falls entirely inside it, entirely before it or entirely after it,
//! and then plans those three runs of statements the same way. A pair that
//! cannot be arranged like that is where the walk gives up: a forward jump
//! that would have to cross the head of a loop the same list jumps back into
//! has nowhere to break to, and that is what a hand-written state machine is
//! made of.
//!
//! # What still needs the CFG
//!
//! * a jump **into** anything — a label inside a loop body, an `if` branch or
//!   a `switch` group, named from outside it; there is no way to enter a Rust
//!   block other than at its top;
//! * a label whose regions would **overlap** another label's without nesting,
//!   which is what a hand-written state machine looks like: `goto` forwards to
//!   a label past one the program also jumps back to;
//! * a **declaration** between the start of a region and its end. The region
//!   is a Rust block, so a `let` inside it would go out of scope at the label
//!   — while C keeps the object alive to the end of the enclosing block. Only
//!   the region's *own* list is affected: a declaration in a block nested
//!   inside the region is exactly as scoped as C says, and leaving that block
//!   through a `break` runs its `cleanup` attributes and frees its variable
//!   length arrays, which is what C asks for too;
//! * `&&label` and the computed `goto *e` it feeds, whose value is a state
//!   number; and
//! * a `case` label that is not a direct child of its `switch` body (Duff's
//!   device), which is not about `goto` at all — see [`crate::sema`].
//!
//! # Two passes over the same shapes
//!
//! The decision has to be made **before** the body is checked, because it
//! changes how sema lowers a `switch` and whether it keeps its labels; the
//! regions themselves can only be built **afterwards**, on the statements. So
//! the walk happens twice — [`analyze`] over the syntax tree and
//! [`restructure`] over the [IR](crate::ir) — and both hand the same
//! planner the same thing: a list of items, each with the labels
//! written on it, the labels the `goto`s inside it name, and whether it
//! declares anything. Statements that do neither are invisible to the plan, so
//! it does not matter that one declaration becomes several `let`s, or that a
//! `static_assert` becomes nothing at all.

use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{self, Function, LabelId, Region, RegionKind, Stmt};

// ---------------------------------------------------------------------------
// the planner
// ---------------------------------------------------------------------------

/// One item of a statement list, as the planner reads it.
struct Item<K> {
    /// The labels written directly on it, outermost first.
    labels: Vec<K>,
    /// Every label of *this* list that a `goto` anywhere inside it names.
    gotos: Vec<K>,
    /// Whether it declares something the rest of the list can see.
    declares: bool,
}

impl<K> Item<K> {
    /// An item with neither a label nor a `goto` in it.
    fn neutral() -> Self {
        Self {
            labels: Vec::new(),
            gotos: Vec::new(),
            declares: false,
        }
    }
}

/// A region that has to be built, and where its `goto`s stand.
#[derive(Clone, Copy)]
struct Need<K> {
    /// Its position in the list the planner was handed, which is what
    /// identifies it once the others are filtered out.
    id: usize,
    label: K,
    kind: RegionKind,
    /// Where the label itself stands.
    anchor: usize,
    /// The first and last items holding a `goto` of this kind to it.
    first: usize,
    last: usize,
}

impl<K> Need<K> {
    /// The narrowest run of items the region has to cover, as `[start, end)`.
    ///
    /// One end is fixed by the label; the other is where the outermost `goto`
    /// stands, and may be widened to make the regions nest.
    fn span(&self) -> (usize, usize) {
        match self.kind {
            RegionKind::Block => (self.first, self.anchor),
            RegionKind::Loop => (self.anchor, self.last + 1),
        }
    }
}

/// The shape a statement list is rebuilt into.
enum Node<K> {
    /// The item at this position, unchanged.
    Item(usize),
    /// A region, holding what falls inside it.
    Region {
        label: K,
        kind: RegionKind,
        body: Vec<Node<K>>,
    },
}

/// Plans the regions of one statement list, or gives up.
///
/// `None` means the list holds a jump the two shapes cannot express, and the
/// whole function it belongs to has to go through the [CFG](crate::cfg).
fn plan<K: Copy + Eq>(items: &[Item<K>]) -> Option<Vec<Node<K>>> {
    let mut needs: Vec<Need<K>> = Vec::new();
    for (anchor, item) in items.iter().enumerate() {
        for label in &item.labels {
            let mut backward: Option<(usize, usize)> = None;
            let mut forward: Option<(usize, usize)> = None;
            for (at, other) in items.iter().enumerate() {
                if !other.gotos.contains(label) {
                    continue;
                }
                let side = if at < anchor {
                    &mut forward
                } else {
                    &mut backward
                };
                match side {
                    Some((first, last)) => {
                        *first = (*first).min(at);
                        *last = (*last).max(at);
                    }
                    None => *side = Some((at, at)),
                }
            }
            for (kind, found) in [(RegionKind::Block, forward), (RegionKind::Loop, backward)] {
                if let Some((first, last)) = found {
                    needs.push(Need {
                        id: needs.len(),
                        label: *label,
                        kind,
                        anchor,
                        first,
                        last,
                    });
                }
            }
        }
    }
    build(items, 0, items.len(), &needs)
}

/// Plans the items in `[lo, hi)`, given the regions still to be built.
///
/// A region either falls entirely inside this range or entirely outside it:
/// one that straddles the boundary would have to cross a block it is not in,
/// which is where the walk gives up.
fn build<K: Copy + Eq>(
    items: &[Item<K>],
    lo: usize,
    hi: usize,
    needs: &[Need<K>],
) -> Option<Vec<Node<K>>> {
    let mut active: Vec<Need<K>> = Vec::new();
    for need in needs {
        let (start, end) = need.span();
        if end <= lo || start >= hi {
            continue;
        }
        if start < lo || end > hi {
            return None;
        }
        active.push(*need);
    }
    if active.is_empty() {
        return Some((lo..hi).map(Node::Item).collect());
    }
    // The outermost region is the one nothing else may contain: the *last*
    // label reached by a forward jump, whose block ends after every other
    // block in the range, or failing that the *first* label a backward jump
    // returns to, whose loop starts before every other loop.
    let blocks = || active.iter().filter(|need| need.kind == RegionKind::Block);
    let loops = || active.iter().filter(|need| need.kind == RegionKind::Loop);
    if let Some(chosen) = blocks().max_by_key(|need| need.anchor).copied() {
        // Every block in the range ends at or before this one, so starting at
        // the earliest jump of any of them is what makes them nest.
        let start = blocks()
            .map(|need| need.first)
            .min()
            .expect("chosen is one");
        if splits(items, lo, hi, start, chosen.anchor, chosen, &active) {
            return assemble(items, lo, hi, start, chosen.anchor, chosen, needs);
        }
    }
    if let Some(chosen) = loops().min_by_key(|need| need.anchor).copied() {
        let mut end = loops()
            .map(|need| need.last + 1)
            .max()
            .expect("chosen is one");
        // A block that begins inside the loop has to end inside it too.
        for need in blocks().filter(|need| need.first >= chosen.anchor) {
            end = end.max(need.anchor);
        }
        if splits(items, lo, hi, chosen.anchor, end, chosen, &active) {
            return assemble(items, lo, hi, chosen.anchor, end, chosen, needs);
        }
    }
    None
}

/// Whether `chosen` may take `[start, end)` out of `[lo, hi)`.
///
/// The two are what makes the walk linear: the choice is checked here and then
/// committed to, so a range is never planned twice.
#[allow(clippy::too_many_arguments)]
fn splits<K: Copy + Eq>(
    items: &[Item<K>],
    lo: usize,
    hi: usize,
    start: usize,
    end: usize,
    chosen: Need<K>,
    active: &[Need<K>],
) -> bool {
    // A region is a Rust block, and a declaration inside one would go out of
    // scope where C keeps it alive. See the module docs.
    if items[start..end].iter().any(|item| item.declares) {
        return false;
    }
    // Every other region has to sit in one of the three parts, whole.
    let part = |need: &Need<K>| {
        let (from, to) = need.span();
        (from >= lo && to <= start) || (from >= start && to <= end) || (from >= end && to <= hi)
    };
    active.iter().all(|need| need.id == chosen.id || part(need))
}

/// Builds `chosen` over `[start, end)` and plans what falls on either side.
#[allow(clippy::too_many_arguments)]
fn assemble<K: Copy + Eq>(
    items: &[Item<K>],
    lo: usize,
    hi: usize,
    start: usize,
    end: usize,
    chosen: Need<K>,
    needs: &[Need<K>],
) -> Option<Vec<Node<K>>> {
    let rest: Vec<Need<K>> = needs
        .iter()
        .filter(|need| need.id != chosen.id)
        .copied()
        .collect();
    let mut out = build(items, lo, start, &rest)?;
    let body = build(items, start, end, &rest)?;
    out.push(Node::Region {
        label: chosen.label,
        kind: chosen.kind,
        body,
    });
    out.extend(build(items, end, hi, &rest)?);
    Some(out)
}

/// The labels the plan built a region for.
fn planned<K: Copy + Eq + std::hash::Hash>(nodes: &[Node<K>], out: &mut HashSet<K>) {
    for node in nodes {
        if let Node::Region { label, body, .. } = node {
            out.insert(*label);
            planned(body, out);
        }
    }
}

// ---------------------------------------------------------------------------
// the syntax tree: which functions keep the structured form
// ---------------------------------------------------------------------------

/// Whether every `goto` in `body` is one the regions can express, and which
/// labels they name.
///
/// `None` is the answer that sends the function through the
/// [CFG](crate::cfg). A `Some` names the labels a region will be built for,
/// which are the ones sema has to keep in the statements it lowers; a label
/// nothing jumps to is not among them and leaves no trace in the output.
pub fn analyze(body: &ast::Block) -> Option<HashSet<String>> {
    let mut scan = Scan {
        hosts: Vec::new(),
        targets: HashSet::new(),
    };
    scan.list(&entries(&body.items))?;
    Some(scan.targets)
}

/// One entry of a statement list, as the syntax-tree walk sees it.
enum Entry<'a> {
    Stmt(&'a ast::Stmt),
    /// A declaration, whose scope is the rest of the list.
    Decl,
    /// A `static_assert` or a nested function definition: nothing that runs.
    Nothing,
}

fn entries(items: &[ast::BlockItem]) -> Vec<Entry<'_>> {
    items
        .iter()
        .map(|item| match item {
            ast::BlockItem::Stmt(stmt) => Entry::Stmt(stmt),
            ast::BlockItem::Decl(_) => Entry::Decl,
            // A nested function is an item of its own: its body is checked
            // apart from this one, and its labels are its own.
            ast::BlockItem::StaticAssert(_) | ast::BlockItem::NestedFunction(_) => Entry::Nothing,
        })
        .collect()
}

struct Scan<'a> {
    /// The labels of every statement list enclosing the one being walked, from
    /// the outermost in. A `goto` may only name one of them.
    hosts: Vec<HashSet<&'a str>>,
    targets: HashSet<String>,
}

impl<'a> Scan<'a> {
    /// A statement list, which is the only place a label may stand.
    fn list(&mut self, entries: &[Entry<'a>]) -> Option<()> {
        let mut hosted: HashSet<&'a str> = HashSet::new();
        for entry in entries {
            if let Entry::Stmt(stmt) = entry {
                for label in labels_of(stmt) {
                    hosted.insert(label);
                }
            }
        }
        let items: Vec<Item<&'a str>> = entries
            .iter()
            .map(|entry| match entry {
                Entry::Stmt(stmt) => {
                    let stmt: &'a ast::Stmt = stmt;
                    let mut gotos = Vec::new();
                    gotos_of(stmt, &hosted, &mut gotos);
                    Item {
                        labels: labels_of(stmt),
                        gotos,
                        declares: false,
                    }
                }
                Entry::Decl => Item {
                    declares: true,
                    ..Item::neutral()
                },
                Entry::Nothing => Item::neutral(),
            })
            .collect();
        let plan = plan(&items)?;
        let mut built: HashSet<&'a str> = HashSet::new();
        planned(&plan, &mut built);
        self.targets
            .extend(built.into_iter().map(ToOwned::to_owned));
        self.hosts.push(hosted);
        for entry in entries {
            if let Entry::Stmt(stmt) = entry {
                self.stmt(stmt)?;
            }
        }
        self.hosts.pop();
        Some(())
    }

    /// The labels of the list being walked, so that a `goto` written in it can
    /// be told from one that leaves for a block it is not in.
    fn knows(&self, name: &str) -> bool {
        self.hosts.iter().any(|hosted| hosted.contains(name))
    }

    /// Walks a statement, checking its jumps and planning the lists inside it.
    fn stmt(&mut self, stmt: &'a ast::Stmt) -> Option<()> {
        match &stmt.kind {
            // A label the walk has not entered the list of is a jump into a
            // block, which nothing here can express.
            ast::StmtKind::Goto(label) => self.knows(&label.name).then_some(()),
            // Its target is a state number; see [`crate::cfg`].
            ast::StmtKind::GotoPtr(_) => None,
            ast::StmtKind::Compound(block) => self.list(&entries(&block.items)),
            // A label chain, a `case` and a `default` all stand on a statement
            // of the list they are written in.
            ast::StmtKind::Labeled { body, .. }
            | ast::StmtKind::Case { body, .. }
            | ast::StmtKind::Default { body } => self.stmt(body),
            ast::StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.stmt(then_branch)?;
                match else_branch {
                    Some(branch) => self.stmt(branch),
                    None => Some(()),
                }
            }
            ast::StmtKind::While { body, .. }
            | ast::StmtKind::DoWhile { body, .. }
            | ast::StmtKind::For { body, .. } => self.stmt(body),
            // The body of a `switch` is split into the groups its labels
            // delimit, so it is not a list a region may span: the statements
            // in it are walked, and a label written there hosts nothing.
            ast::StmtKind::Switch { body, .. } => match &body.kind {
                ast::StmtKind::Compound(block) => {
                    for item in &block.items {
                        if let ast::BlockItem::Stmt(stmt) = item {
                            self.stmt(stmt)?;
                        }
                    }
                    Some(())
                }
                _ => self.stmt(body),
            },
            _ => Some(()),
        }
    }
}

/// The labels written directly on a statement, outermost first.
fn labels_of(stmt: &ast::Stmt) -> Vec<&str> {
    let mut out = Vec::new();
    let mut current = stmt;
    loop {
        match &current.kind {
            ast::StmtKind::Labeled { label, body } => {
                out.push(label.name.as_str());
                current = body;
            }
            // `case 1: found: …` — the label under it is written on the same
            // statement of the same list.
            ast::StmtKind::Case { body, .. } | ast::StmtKind::Default { body } => current = body,
            _ => return out,
        }
    }
}

/// Every label of `hosted` that a `goto` inside `stmt` names.
fn gotos_of<'a>(stmt: &'a ast::Stmt, hosted: &HashSet<&'a str>, out: &mut Vec<&'a str>) {
    match &stmt.kind {
        ast::StmtKind::Goto(label) => {
            if let Some(name) = hosted.get(label.name.as_str())
                && !out.contains(name)
            {
                out.push(*name);
            }
        }
        ast::StmtKind::Labeled { body, .. }
        | ast::StmtKind::Case { body, .. }
        | ast::StmtKind::Default { body }
        | ast::StmtKind::While { body, .. }
        | ast::StmtKind::DoWhile { body, .. }
        | ast::StmtKind::For { body, .. }
        | ast::StmtKind::Switch { body, .. } => gotos_of(body, hosted, out),
        ast::StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            gotos_of(then_branch, hosted, out);
            if let Some(branch) = else_branch {
                gotos_of(branch, hosted, out);
            }
        }
        ast::StmtKind::Compound(block) => {
            for item in &block.items {
                // A nested function's `goto`s are its own.
                if let ast::BlockItem::Stmt(stmt) = item {
                    gotos_of(stmt, hosted, out);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// the statements: building the regions
// ---------------------------------------------------------------------------

/// Wraps the statements of `body` in the regions [`analyze`] promised.
///
/// `names` is what the generated labels are named after, and `functions` is
/// the program's function table, which [`ir::always_terminates`] reads a
/// `_Noreturn` call through. The answer is whether every list could be
/// planned: it is `true` whenever `analyze` said so of the same function, and
/// a `false` means the two disagree, which only a `goto` sema had to drop for
/// an error of its own can cause.
pub fn restructure(
    body: &mut Vec<Stmt>,
    names: &HashMap<LabelId, String>,
    functions: &[Function],
) -> bool {
    let mut rebuild = Rebuild {
        names,
        functions,
        planned: true,
    };
    rebuild.list(body);
    rebuild.planned
}

struct Rebuild<'a> {
    names: &'a HashMap<LabelId, String>,
    functions: &'a [Function],
    planned: bool,
}

impl Rebuild<'_> {
    /// Rebuilds one statement list, innermost first.
    fn list(&mut self, stmts: &mut Vec<Stmt>) {
        for stmt in stmts.iter_mut() {
            self.stmt(stmt);
        }
        // Only a list with a label in it can hold a region, and sema keeps a
        // label exactly when something jumps to it.
        if !stmts.iter().any(|stmt| !ir_labels_of(stmt).is_empty()) {
            return;
        }
        let items: Vec<Item<LabelId>> = stmts
            .iter()
            .map(|stmt| {
                let mut gotos = Vec::new();
                ir_gotos_of(stmt, &mut gotos);
                Item {
                    labels: ir_labels_of(stmt),
                    gotos,
                    declares: matches!(stmt, Stmt::Let { .. } | Stmt::Vla(_) | Stmt::Cleanup(_)),
                }
            })
            .collect();
        let Some(nodes) = plan(&items) else {
            self.planned = false;
            return;
        };
        let mut ranges: HashMap<LabelId, SourceRange> = HashMap::new();
        for stmt in stmts.iter() {
            ir_label_ranges(stmt, &mut ranges);
        }
        let mut slots: Vec<Option<Stmt>> = std::mem::take(stmts).into_iter().map(Some).collect();
        *stmts = self.apply(nodes, &mut slots, &ranges);
    }

    /// Turns a plan back into statements.
    fn apply(
        &self,
        nodes: Vec<Node<LabelId>>,
        slots: &mut Vec<Option<Stmt>>,
        ranges: &HashMap<LabelId, SourceRange>,
    ) -> Vec<Stmt> {
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            match node {
                Node::Item(at) => out.push(slots[at].take().expect("one node per statement")),
                Node::Region { label, kind, body } => {
                    let body = self.apply(body, slots, ranges);
                    let falls_out =
                        kind == RegionKind::Loop && !ir::always_terminates(&body, self.functions);
                    out.push(Stmt::Region(Box::new(Region {
                        label,
                        name: self.names.get(&label).cloned().unwrap_or_default(),
                        kind,
                        body,
                        falls_out,
                        range: ranges.get(&label).copied().unwrap_or(SourceRange::at(0)),
                    })));
                }
            }
        }
        out
    }

    /// Rebuilds the lists inside a statement.
    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Block(items) => self.list(items),
            Stmt::Label { body, .. } | Stmt::Case { body, .. } => self.stmt(body),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.stmt(then_branch);
                if let Some(branch) = else_branch {
                    self.stmt(branch);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => self.stmt(body),
            Stmt::For { init, body, .. } => {
                for stmt in init {
                    self.stmt(stmt);
                }
                self.stmt(body);
            }
            // The groups of a `switch` are not lists a region may span; the
            // statements in them are rebuilt all the same.
            Stmt::Switch(switch) => {
                for stmt in &mut switch.prelude {
                    self.stmt(stmt);
                }
                for group in &mut switch.groups {
                    for stmt in &mut group.body {
                        self.stmt(stmt);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The labels written directly on a statement, outermost first.
fn ir_labels_of(stmt: &Stmt) -> Vec<LabelId> {
    let mut out = Vec::new();
    let mut current = stmt;
    loop {
        match current {
            Stmt::Label { id, body, .. } => {
                out.push(*id);
                current = body;
            }
            Stmt::Case { body, .. } => current = body,
            _ => return out,
        }
    }
}

/// Where each of them was written.
fn ir_label_ranges(stmt: &Stmt, out: &mut HashMap<LabelId, SourceRange>) {
    let mut current = stmt;
    loop {
        match current {
            Stmt::Label { id, body, range } => {
                out.insert(*id, *range);
                current = body;
            }
            Stmt::Case { body, .. } => current = body,
            _ => return,
        }
    }
}

/// Every label a `goto` inside `stmt` names.
fn ir_gotos_of(stmt: &Stmt, out: &mut Vec<LabelId>) {
    match stmt {
        Stmt::Goto { id, .. } => {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        Stmt::Block(items) => {
            for stmt in items {
                ir_gotos_of(stmt, out);
            }
        }
        Stmt::Region(region) => {
            for stmt in &region.body {
                ir_gotos_of(stmt, out);
            }
        }
        Stmt::Label { body, .. }
        | Stmt::Case { body, .. }
        | Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. } => ir_gotos_of(body, out),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            ir_gotos_of(then_branch, out);
            if let Some(branch) = else_branch {
                ir_gotos_of(branch, out);
            }
        }
        Stmt::For { init, body, .. } => {
            for stmt in init {
                ir_gotos_of(stmt, out);
            }
            ir_gotos_of(body, out);
        }
        Stmt::Switch(switch) => {
            for stmt in switch
                .prelude
                .iter()
                .chain(switch.groups.iter().flat_map(|group| group.body.iter()))
            {
                ir_gotos_of(stmt, out);
            }
        }
        Stmt::SwitchTree(switch) => ir_gotos_of(&switch.body, out),
        _ => {}
    }
}
