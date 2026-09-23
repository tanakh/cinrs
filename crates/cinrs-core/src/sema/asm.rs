//! GNU inline assembly, mapped onto `core::arch::asm!`.
//!
//! GCC's extended asm and Rust's `asm!` share one model — an opaque template
//! plus operands with constraints — so for the shapes real C writes (`rdtsc`,
//! `pause`, a bit scan, an `xchg`, `asm volatile("" ::: "memory")`) the
//! translation is mechanical. This module does all of it and leaves code
//! generation a finished [`ir::AsmStmt`]; everything it cannot map is a
//! diagnostic that names the constraint or the feature, because an `asm!`
//! whose meaning differs from GCC's would be worse than none.
//!
//! # The mapping
//!
//! | GCC                                | `asm!`                                  |
//! |------------------------------------|-----------------------------------------|
//! | `"r"`, `"g"`, `"rm"`, `"ri"`       | `in(reg)`; `reg_byte` for an 8-bit value |
//! | `"=r"` / `"=&r"` / `"+r"`          | `lateout` / `out` / `inout`             |
//! | `"q"`, `"Q"`                       | as `"r"` (`reg_abcd` on 32-bit x86)     |
//! | `"a" "c" "d" "S" "D"`              | `in("eax")` etc., at the operand's width |
//! | `"b"` / `"=b"` / `"+b"`            | a scratch `inout(reg) v => _` / `out(reg)` / `inout(reg)`, swapped with rbx by an `xchg` either side of the template (below) |
//! | `"x"`                              | `xmm_reg`                               |
//! | `"i"`, `"n"`                       | `const`, folded; written `${oN}`        |
//! | `"0"` … (tied to an output)        | `inout(…) input => output`              |
//! | `%N`, `%[name]`                    | `{oN}` at the operand's width (`{oN:e}` for 32 bits, `:x` for 16), or the register for an explicit one |
//! | an operand the template never names | `{oN}` in a trailing `/* … */` comment |
//! | `%kN` `%wN` `%bN` `%hN` `%qN`      | `{oN:e}` `:x` `:l` `:h` (`reg_abcd`) `:r` |
//! | `%%`, `%{`, `%}`, `%\|`            | `%`, `{{`, `}}`, `\|`                   |
//! | clobber `"rax"`, `"xmm0"`          | `out("rax") _`                          |
//! | clobber `"memory"`, `"cc"`         | nothing: `asm!` assumes both            |
//!
//! Every operand the template can refer to is *named* — `o` and its GCC
//! number — because `asm!` refuses a positional operand after an explicit
//! register one, and GCC puts no such order on its operands. An explicit
//! register cannot be referred to from an `asm!` template at all, so a `%0`
//! that names one is replaced by the register itself, at the width the
//! modifier asks for. `options(att_syntax)` is always given (GCC's x86
//! template is AT&T), and no other option: `volatile`, "memory is read and
//! written" and "flags are clobbered" are `asm!`'s defaults and GCC's most
//! conservative reading, so `pure`, `nomem`, `readonly`, `preserves_flags`
//! and `nostack` are never added. Where a constraint allows a register *or*
//! memory (`"rm"`, `"g"`), the register is chosen: the instruction GCC would
//! pick may differ, the meaning does not.
//!
//! # The `"b"` constraint
//!
//! `rustc` refuses rbx as an operand, so a `"b"` operand is carried in a
//! scratch register the compiler chooses, named like any other (`oN`), and
//! swapped with rbx by an `xchg` on either side of the template — on x86-64
//! `xchgq %rbx, {oN:r}`, on 32-bit x86 `xchgl %ebx, {oN:e}`. This is GCC's
//! own `<cpuid.h>` idiom done for the user:
//!
//! * the first `xchg` puts the scratch's value in rbx and keeps rbx's in the
//!   scratch, so an input (`"b"`, `"+b"`, or a `"0"` tied to a `"=b"`) is in
//!   rbx while the template runs;
//! * the second puts back what rbx held and leaves in the scratch what the
//!   template left in rbx, which is the output of `"=b"` and `"+b"`;
//! * for an input alone the scratch's final value is thrown away:
//!   `inout(reg) v => _`.
//!
//! The first `xchg` writes the scratch after every input has been loaded and
//! before the template reads any, so the scratch must not share a register
//! with an input: an output-only `"=b"` is `out(reg)` (early clobber), never
//! `lateout`, and the other forms are `inout`, which never share. A `%0` that
//! names the operand is written as rbx at the width asked for (`%ebx`, `%bx`,
//! `%bl`, `%bh` for `%h0`, `%rbx`), exactly as an explicit register is. One
//! `"b"` operand per statement, as in GCC, and not together with an `rbx`
//! clobber; a template that writes `%rbx` itself as well is its own business.
//! A one-byte `"b"` operand is refused: `reg_byte` takes no `:r` modifier,
//! and an `xchgb` would leave the rest of rbx unrestored.
//!
//! # What `rustc` says (probed on x86-64 with 1.88+)
//!
//! * `rbx`, `ebx`, `bx`, `bl` cannot be operands *or* clobbers — "rbx is used
//!   internally by LLVM" — so `"b"` is the scratch-and-`xchg` above and an
//!   `rbx` clobber is refused here, pointing at `"b"`. The template may still
//!   *mention* `%rbx` when it restores it: GCC's own `<cpuid.h>` idiom
//!   `xchgq %rbx, %q1; cpuid; xchgq %rbx, %q1` with `"=&r"` works unchanged.
//! * A positional operand cannot follow a named or an explicit-register one
//!   (hence the names).
//! * An explicit register has to be spelled at the value's width: `al` for a
//!   `u8` (`inout("rax")` with a `u8` is "type `u8` cannot be used with this
//!   register class"); `ax`, `ecx`, `sil` all work.
//! * `reg` takes 16-, 32- and 64-bit integers, `f32`, `f64` and pointers, but
//!   not 8-bit values, which need `reg_byte`; `reg_byte` takes no modifier.
//! * `:e`, `:x`, `:l`, `:r` work on `reg`; `:h` only on `reg_abcd`.
//! * With no modifier a `reg` operand is printed as the *whole* register
//!   (`rax`) whatever its type — rustc warns (`asm_sub_register`) — where GCC
//!   prints it at the operand's width. So a plain `%0` on an `int` is
//!   `{o0:e}`, or `addl %0, %1` would assemble as `addl %rcx, %rax`.
//! * A named operand the template never mentions is an error ("named
//!   argument never used"); GCC takes such operands without a word, so they
//!   are mentioned in an assembler comment at the end of the template.
//! * An output may be any place expression, a member of a packed record
//!   included: `lateout(reg) (*q).v` compiles and stores unaligned.
//! * A `const` operand is substituted as a bare number, so AT&T's `$` has to
//!   be in the template: `${o1}`.
//! * `xmm_reg` takes `f32`, `f64`, 32- and 64-bit integers and the 128-bit
//!   vector types.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{self, AsmOperandKind, AsmReg, ConstValue, Place, Stmt, Ty};
use crate::target::Arch;

use super::Sema;

/// What a constraint asked for, once the alternative has been chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    /// `r`, `g`, `q`, `Q`: any general-purpose register.
    General,
    /// `q`, `Q`: a register with an addressable low byte.
    Byte,
    /// `x`: an SSE register.
    Xmm,
    /// `a`, `c`, `d`, `S`, `D`: that register.
    Explicit(char),
    /// `b`: rbx, through a scratch register and an `xchg` either side.
    Rbx,
    /// `i`, `n`: an integer constant.
    Imm,
    /// A digit: the same location as that output operand.
    Tie(usize),
}

/// A parsed constraint string.
struct Constraint {
    choice: Choice,
    /// `+`: read and written.
    plus: bool,
    /// `&`: written before every input has been read.
    early: bool,
}

/// A piece of a parsed extended template.
enum Piece {
    Text(String),
    Ref {
        modifier: Option<char>,
        target: RefTarget,
    },
}

enum RefTarget {
    Number(usize),
    Name(String),
}

/// What a GCC operand number stands for once the operands are built.
#[derive(Clone, Copy)]
enum Slot {
    /// This entry of [`ir::AsmStmt::operands`].
    Operand(usize),
    /// Reported already.
    Failed,
}

/// The general-purpose register families: the name at each width (8, 16,
/// 32, 64 bits), the 8-bit high name if there is one, and whether it is only
/// on x86-64.
struct Family {
    names: [&'static str; 4],
    high: Option<&'static str>,
    x86_64_only: bool,
}

const FAMILIES: &[Family] = &[
    fam(["al", "ax", "eax", "rax"], Some("ah"), false),
    fam(["cl", "cx", "ecx", "rcx"], Some("ch"), false),
    fam(["dl", "dx", "edx", "rdx"], Some("dh"), false),
    fam(["sil", "si", "esi", "rsi"], None, false),
    fam(["dil", "di", "edi", "rdi"], None, false),
    fam(["r8b", "r8w", "r8d", "r8"], None, true),
    fam(["r9b", "r9w", "r9d", "r9"], None, true),
    fam(["r10b", "r10w", "r10d", "r10"], None, true),
    fam(["r11b", "r11w", "r11d", "r11"], None, true),
    fam(["r12b", "r12w", "r12d", "r12"], None, true),
    fam(["r13b", "r13w", "r13d", "r13"], None, true),
    fam(["r14b", "r14w", "r14d", "r14"], None, true),
    fam(["r15b", "r15w", "r15d", "r15"], None, true),
];

const fn fam(names: [&'static str; 4], high: Option<&'static str>, x86_64_only: bool) -> Family {
    Family {
        names,
        high,
        x86_64_only,
    }
}

/// rbx, which is not in [`FAMILIES`] because it is never an `asm!` operand:
/// only a `"b"` operand's references in the template are written with it.
const RBX: Family = fam(["bl", "bx", "ebx", "rbx"], Some("bh"), false);

const XMM: [&str; 16] = [
    "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7", "xmm8", "xmm9", "xmm10",
    "xmm11", "xmm12", "xmm13", "xmm14", "xmm15",
];

/// Why a register `rustc` keeps for itself cannot be named, or `None`.
fn reserved_register(name: &str) -> Option<&'static str> {
    match name {
        "rbx" | "ebx" | "bx" | "bl" | "bh" => Some(
            "rustc reserves rbx (LLVM uses it internally) and refuses it as an 'asm!' clobber; \
             give the value the instruction leaves in rbx a \"=b\" operand instead, which cinrs \
             carries in and out of rbx with an 'xchg' around the template",
        ),
        "rsp" | "esp" | "sp" | "spl" => Some("the stack pointer cannot be an 'asm!' operand"),
        "rbp" | "ebp" | "bp" | "bpl" => Some("the frame pointer cannot be an 'asm!' operand"),
        _ => None,
    }
}

impl Sema<'_> {
    /// Checks an `asm` statement and maps it onto `asm!`.
    ///
    /// Anything that does not map is reported and the statement becomes a
    /// no-op; the unit then fails to compile, so nothing is silently lost.
    pub(super) fn asm_stmt(&mut self, asm: &ast::AsmStmt, range: SourceRange) -> Stmt {
        if !matches!(self.target.arch, Arch::X86 | Arch::X86_64) {
            self.error(
                range,
                format!(
                    "inline assembly is only supported on x86 and x86-64: the template is \
                     assembly for one architecture and the operands are mapped onto x86's \
                     registers, and the target here is {}",
                    self.target.arch.as_str()
                ),
            );
            return Stmt::Nop;
        }
        if let Some(frame) = self.nest.last() {
            let func = self.program.function(frame.func);
            if func.is_safe() {
                let name = func.name.clone();
                self.error(
                    range,
                    format!(
                        "inline assembly cannot be written in the safe function '{name}': Rust's \
                         'asm!' is unsafe, and a safe function has no 'unsafe' block to put it \
                         in. Drop [[cinrs::safe]] from '{name}'"
                    ),
                );
                return Stmt::Nop;
            }
        }
        if asm.goto {
            self.error(
                range,
                "'asm goto' is not supported yet: Rust's 'asm!' has 'label' blocks, but the \
                 jump to a C label has to go through the function's control flow, which this \
                 release does not do. Write the branch in C on a flag the 'asm' sets",
            );
            return Stmt::Nop;
        }
        if asm.template.node.trim_start().starts_with(".intel_syntax") {
            self.error(
                asm.template.range,
                "a template that switches to Intel syntax with '.intel_syntax' is not supported: \
                 GCC's x86 templates are AT&T, and cinrs gives 'asm!' options(att_syntax) to \
                 match. Write the instructions in AT&T syntax",
            );
            return Stmt::Nop;
        }
        if !asm.extended {
            let Some(template) = self.basic_template(&asm.template) else {
                return Stmt::Nop;
            };
            return Stmt::Asm(Box::new(ir::AsmStmt {
                template,
                operands: Vec::new(),
                clobbers: Vec::new(),
                range,
            }));
        }
        self.extended_asm(asm, range)
            .map_or(Stmt::Nop, |stmt| Stmt::Asm(Box::new(stmt)))
    }

    /// A basic asm template: `%` is literal, and only the braces `asm!` would
    /// read as operands need escaping — which GCC's dialect alternatives
    /// make a refusal instead.
    fn basic_template(&mut self, template: &ast::Spanned<String>) -> Option<String> {
        if template.node.contains(['{', '}']) {
            self.dialect_alternatives(template.range);
            return None;
        }
        Some(template.node.clone())
    }

    fn dialect_alternatives(&mut self, range: SourceRange) {
        self.error(
            range,
            "'{' and '}' in an 'asm' template are GCC's assembler dialect alternatives \
             ('{att|intel}'), which are not supported: write the AT&T form alone, or '%{' and \
             '%}' for a literal brace",
        );
    }

    fn extended_asm(&mut self, asm: &ast::AsmStmt, range: SourceRange) -> Option<ir::AsmStmt> {
        let x86_64 = self.target.arch == Arch::X86_64;
        let pieces = self.parse_template(&asm.template);
        let mut failed = pieces.is_none();
        let mut operands: Vec<ir::AsmOperand> = Vec::new();
        let mut slots: Vec<Slot> = Vec::new();
        // An input tied to an output: which output, so that a second tie is
        // caught.
        let mut tied: Vec<bool> = Vec::new();
        // The GCC number of the operand with the constraint `"b"`.
        let mut rbx: Option<usize> = None;

        for (index, operand) in asm.outputs.iter().enumerate() {
            match self.asm_output(operand, index, x86_64, &mut rbx) {
                Some(op) => {
                    slots.push(Slot::Operand(operands.len()));
                    operands.push(op);
                }
                None => {
                    failed = true;
                    slots.push(Slot::Failed);
                }
            }
            tied.push(false);
        }
        for (offset, operand) in asm.inputs.iter().enumerate() {
            let index = asm.outputs.len() + offset;
            match self.asm_input(operand, index, &slots, &mut operands, &mut tied, &mut rbx) {
                Some(slot) => slots.push(slot),
                None => {
                    failed = true;
                    slots.push(Slot::Failed);
                }
            }
        }

        let mut clobbers: Vec<&'static str> = Vec::new();
        for clobber in &asm.clobbers {
            match self.asm_clobber(clobber, x86_64, &operands, rbx) {
                Some(Some(reg)) if !clobbers.contains(&reg) => clobbers.push(reg),
                Some(_) => {}
                None => failed = true,
            }
        }

        let names: Vec<Option<&str>> = asm
            .outputs
            .iter()
            .chain(&asm.inputs)
            .map(|operand| operand.name.as_ref().map(|name| name.name.as_str()))
            .collect();
        // `claim_rbx` runs only once the operand is built, so its slot is
        // an operand.
        let rbx_op = rbx.and_then(|index| match slots.get(index) {
            Some(Slot::Operand(op)) => Some(*op),
            _ => None,
        });
        let template = match pieces {
            Some(pieces) => self.render_template(
                &pieces,
                &asm.template,
                &names,
                &slots,
                &mut operands,
                rbx_op,
            ),
            None => None,
        };
        if failed {
            return None;
        }
        // The `"b"` operand's scratch, swapped with rbx either side of the
        // template; see the module documentation.
        let template = match (template, rbx_op) {
            (Some(template), Some(op)) => {
                let name = operands[op].name.as_deref().unwrap_or_default();
                let xchg = if x86_64 {
                    format!("xchgq %rbx, {{{name}:r}}")
                } else {
                    format!("xchgl %ebx, {{{name}:e}}")
                };
                Some(format!("{xchg}\n{template}\n{xchg}"))
            }
            (template, _) => template,
        };
        Some(ir::AsmStmt {
            template: template?,
            operands,
            clobbers,
            range,
        })
    }

    fn asm_output(
        &mut self,
        operand: &ast::AsmOperand,
        index: usize,
        x86_64: bool,
        rbx: &mut Option<usize>,
    ) -> Option<ir::AsmOperand> {
        let constraint = self.parse_constraint(&operand.constraint, true)?;
        let place = self.lvalue_assignable(&operand.expr)?;
        if self.bit_field_of(&place).is_some() {
            self.error(
                operand.expr.range,
                "a bit-field cannot be an 'asm' output: it has no register-sized storage of its \
                 own. Write the output to a local and assign the bit-field from it",
            );
            return None;
        }
        let ty = place.ty;
        let reg = self.asm_register(constraint.choice, ty, operand, x86_64)?;
        let in_rbx = constraint.choice == Choice::Rbx;
        if in_rbx {
            self.claim_rbx(rbx, index, operand)?;
        }
        let kind = if constraint.plus {
            AsmOperandKind::InOut {
                input: None,
                output: place,
            }
        } else {
            // The first `xchg` writes a `"b"` operand's scratch before the
            // template reads its inputs: an early clobber whatever the `&`.
            AsmOperandKind::Out {
                place,
                late: !constraint.early && !in_rbx,
            }
        };
        Some(ir::AsmOperand {
            name: operand_name(reg, index),
            reg,
            kind,
            ty,
            range: operand.expr.range,
        })
    }

    fn asm_input(
        &mut self,
        operand: &ast::AsmOperand,
        index: usize,
        slots: &[Slot],
        operands: &mut Vec<ir::AsmOperand>,
        tied: &mut [bool],
        rbx: &mut Option<usize>,
    ) -> Option<Slot> {
        let x86_64 = self.target.arch == Arch::X86_64;
        let constraint = self.parse_constraint(&operand.constraint, false)?;
        let value = self.expr(&operand.expr)?;
        if value.ty.is_error() {
            return None;
        }
        match constraint.choice {
            Choice::Tie(target) => {
                let at = operand.constraint.range;
                let Some(slot) = slots.get(target).copied() else {
                    self.error(
                        at,
                        format!(
                            "the constraint \"{}\" ties this input to operand {target}, which is \
                             not an output",
                            operand.constraint.node
                        ),
                    );
                    return None;
                };
                let Slot::Operand(op) = slot else {
                    return None;
                };
                if tied[target] {
                    self.error(
                        at,
                        format!("operand {target} is tied to more than one input"),
                    );
                    return None;
                }
                let output = &operands[op];
                let AsmOperandKind::Out { place, .. } = &output.kind else {
                    self.error(
                        at,
                        format!(
                            "operand {target} is read and written ('+') already, so no input \
                             can be tied to it"
                        ),
                    );
                    return None;
                };
                let place: Place = place.clone();
                let value = self.convert(value, output.ty);
                tied[target] = true;
                operands[op].kind = AsmOperandKind::InOut {
                    input: Some(value),
                    output: place,
                };
                Some(Slot::Operand(op))
            }
            Choice::Imm => {
                let ty = value.ty;
                let folded = match self.const_eval(&value) {
                    Some(ConstValue::Int(v)) if ty.is_integer() => ty.wrap(v, &self.target),
                    _ => {
                        self.error(
                            operand.expr.range,
                            format!(
                                "the constraint \"{}\" asks for an immediate, and this operand \
                                 is not an integer constant expression: 'asm!' takes an \
                                 immediate as a 'const' operand. Write a constant, or use \"r\" \
                                 to pass the value in a register",
                                operand.constraint.node
                            ),
                        );
                        return None;
                    }
                };
                operands.push(ir::AsmOperand {
                    name: Some(format!("o{index}")),
                    reg: AsmReg::Class("reg"),
                    kind: AsmOperandKind::Const(folded),
                    ty,
                    range: operand.expr.range,
                });
                Some(Slot::Operand(operands.len() - 1))
            }
            choice => {
                let ty = value.ty;
                let reg = self.asm_register(choice, ty, operand, x86_64)?;
                // A `"b"` input is loaded into the scratch, which the `xchg`s
                // then overwrite: what is left there afterwards is not wanted.
                let kind = if choice == Choice::Rbx {
                    self.claim_rbx(rbx, index, operand)?;
                    AsmOperandKind::Scratch(value)
                } else {
                    AsmOperandKind::In(value)
                };
                operands.push(ir::AsmOperand {
                    name: operand_name(reg, index),
                    reg,
                    kind,
                    ty,
                    range: operand.expr.range,
                });
                Some(Slot::Operand(operands.len() - 1))
            }
        }
    }

    /// Records operand `index` as the statement's `"b"` operand, or reports
    /// that another one is there already.
    fn claim_rbx(
        &mut self,
        rbx: &mut Option<usize>,
        index: usize,
        operand: &ast::AsmOperand,
    ) -> Option<()> {
        if let Some(first) = *rbx {
            self.error(
                operand.constraint.range,
                format!(
                    "operand {index} cannot be in \"b\" too: operand {first} is in rbx already, \
                     and one register holds one operand"
                ),
            );
            return None;
        }
        *rbx = Some(index);
        Some(())
    }

    /// Parses a constraint string, choosing among its alternatives, and
    /// reports what cannot be mapped.
    fn parse_constraint(
        &mut self,
        constraint: &ast::Spanned<String>,
        output: bool,
    ) -> Option<Constraint> {
        let text = constraint.node.as_str();
        let at = constraint.range;
        let written = text.starts_with('=');
        let plus = text.starts_with('+');
        if output && !written && !plus {
            self.error(
                at,
                format!("the output constraint \"{text}\" has to start with '=' or '+'"),
            );
            return None;
        }
        if !output && (written || plus) {
            self.error(
                at,
                format!("the input constraint \"{text}\" cannot start with '=' or '+'"),
            );
            return None;
        }
        let body = text.trim_start_matches(['=', '+']);
        if body.starts_with('@') {
            self.error(
                at,
                format!(
                    "the flag output \"{text}\" is not supported: 'asm!' has no flag outputs. \
                     Set a byte register from the flag in the template ('setz %b0') and use \
                     \"=q\""
                ),
            );
            return None;
        }
        let mut early = false;
        let mut best: Option<Choice> = None;
        let mut first_refusal: Option<String> = None;
        for alternative in body.split(',') {
            let mut choice: Option<Choice> = None;
            let mut imm = false;
            let mut tie: Option<usize> = None;
            let mut refusal: Option<String> = None;
            let mut chars = alternative.chars().peekable();
            while let Some(c) = chars.next() {
                let letter = match c {
                    '&' => {
                        early = true;
                        continue;
                    }
                    // Commutative, disparaging and hint characters say
                    // nothing about where the operand lives.
                    '%' | '*' | '?' | '!' | '#' | ' ' | '\t' => continue,
                    'r' | 'g' => Some(Choice::General),
                    'q' | 'Q' => Some(Choice::Byte),
                    'x' => Some(Choice::Xmm),
                    'a' | 'c' | 'd' | 'S' | 'D' => Some(Choice::Explicit(c)),
                    'b' => Some(Choice::Rbx),
                    'i' | 'n' => {
                        imm = true;
                        None
                    }
                    '0'..='9' => {
                        let mut digits = String::from(c);
                        while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                            digits.push(d);
                            chars.next();
                        }
                        tie = digits.parse().ok();
                        None
                    }
                    // A register-or-memory alternative only offers memory when
                    // nothing better is in the same alternative.
                    'm' | 'o' | 'V' | '<' | '>' | 'p' => {
                        refusal.get_or_insert_with(|| memory_refusal(text));
                        None
                    }
                    'Y' => {
                        let second = chars.next().map(String::from).unwrap_or_default();
                        refusal.get_or_insert_with(|| {
                            format!(
                                "the constraint \"Y{second}\" is not supported: 'asm!' has no \
                                 class for it. Use \"x\" for an SSE register"
                            )
                        });
                        None
                    }
                    other => {
                        refusal.get_or_insert_with(|| letter_refusal(other));
                        None
                    }
                };
                if let Some(letter) = letter
                    && choice.is_none()
                {
                    choice = Some(letter);
                }
            }
            let resolved = if output {
                choice
            } else {
                choice
                    .or(tie.map(Choice::Tie))
                    .or(imm.then_some(Choice::Imm))
            };
            // An output cannot be an immediate or tie to another operand.
            let resolved = match resolved {
                None if output && (imm || tie.is_some()) => {
                    refusal.get_or_insert_with(|| {
                        format!("the output constraint \"{text}\" has no register alternative")
                    });
                    None
                }
                other => other,
            };
            match resolved {
                Some(choice) if best.is_none() => best = Some(choice),
                Some(_) => {}
                None => {
                    if first_refusal.is_none() {
                        first_refusal = refusal;
                    }
                }
            }
        }
        let Some(choice) = best else {
            let message = first_refusal
                .unwrap_or_else(|| format!("the constraint \"{text}\" names no operand location"));
            self.error(at, message);
            return None;
        };
        Some(Constraint {
            choice,
            plus,
            early,
        })
    }

    /// Where a register operand of type `ty` lives, or a diagnostic.
    fn asm_register(
        &mut self,
        choice: Choice,
        ty: Ty,
        operand: &ast::AsmOperand,
        x86_64: bool,
    ) -> Option<AsmReg> {
        let at = operand.expr.range;
        let constraint = &operand.constraint.node;
        let size = match self.asm_value_size(ty) {
            Ok(size) => size,
            Err(message) => {
                self.error(at, message);
                return None;
            }
        };
        let word = if x86_64 { 8 } else { 4 };
        let fail = |sema: &mut Self, message: String| {
            sema.error(at, message);
            None
        };
        match choice {
            Choice::General | Choice::Byte => {
                if ty.is_vector() {
                    return fail(
                        self,
                        format!(
                            "a vector operand needs an SSE register: write \"x\", not \
                             \"{constraint}\""
                        ),
                    );
                }
                if size > word {
                    return fail(
                        self,
                        format!(
                            "this {}-byte operand does not fit a general-purpose register on \
                             this target",
                            size
                        ),
                    );
                }
                Some(match size {
                    1 => AsmReg::Class("reg_byte"),
                    _ if choice == Choice::Byte && !x86_64 => AsmReg::Class("reg_abcd"),
                    _ => AsmReg::Class("reg"),
                })
            }
            Choice::Xmm => {
                if (ty.is_integer() && size < 4) || size > 16 {
                    return fail(
                        self,
                        format!(
                            "a {size}-byte operand cannot live in an SSE register (\"x\"): \
                             'asm!' takes 32- and 64-bit values and the 128-bit vector types"
                        ),
                    );
                }
                Some(AsmReg::Class("xmm_reg"))
            }
            Choice::Explicit(letter) => {
                if ty.is_vector() {
                    return fail(
                        self,
                        format!("a vector operand cannot live in \"{letter}\": write \"x\""),
                    );
                }
                let family = match letter {
                    'a' => &FAMILIES[0],
                    'c' => &FAMILIES[1],
                    'd' => &FAMILIES[2],
                    'S' => &FAMILIES[3],
                    _ => &FAMILIES[4],
                };
                let width = match size {
                    1 => 0,
                    2 => 1,
                    4 => 2,
                    8 if x86_64 => 3,
                    _ => {
                        return fail(
                            self,
                            format!(
                                "this {size}-byte operand does not fit the register \
                                 \"{letter}\" names on this target"
                            ),
                        );
                    }
                };
                if width == 0 && !x86_64 && matches!(letter, 'S' | 'D') {
                    return fail(
                        self,
                        format!("\"{letter}\" has no 8-bit form on 32-bit x86"),
                    );
                }
                Some(AsmReg::Explicit(family.names[width]))
            }
            // The scratch register the `xchg` swaps with rbx: a `reg`, whose
            // `:r` (`:e` on 32-bit x86) is the whole register whatever the
            // value's width.
            Choice::Rbx => {
                if ty.is_vector() {
                    return fail(
                        self,
                        "a vector operand cannot live in \"b\": write \"x\"".to_owned(),
                    );
                }
                if size == 1 {
                    return fail(
                        self,
                        "a one-byte operand in \"b\" is not supported: cinrs carries a \"b\" \
                         operand in a scratch register swapped with rbx by an 'xchg', and a byte \
                         register would restore only bl. Widen the operand to 'unsigned int'"
                            .to_owned(),
                    );
                }
                if size > word {
                    return fail(
                        self,
                        format!(
                            "this {size}-byte operand does not fit the register \"b\" names on \
                             this target"
                        ),
                    );
                }
                Some(AsmReg::Class("reg"))
            }
            Choice::Imm | Choice::Tie(_) => unreachable!("handled by the caller"),
        }
    }

    /// The size of an operand's type, if `asm!` has a register type for it.
    fn asm_value_size(&self, ty: Ty) -> Result<u64, String> {
        if ty.is_bool() {
            return Err(
                "a '_Bool' operand has no register type in 'asm!': use 'unsigned char' \
                        and convert"
                    .to_owned(),
            );
        }
        if ty.is_int128() {
            return Err(
                "a 128-bit integer does not fit a register: split it into two 64-bit \
                        operands"
                    .to_owned(),
            );
        }
        let fits = ty.is_integer() || ty.is_pointer() || ty.is_floating() || ty.is_vector();
        let size = self.size_of(ty).filter(|_| fits);
        match size {
            Some(size) if !ty.is_floating() || size == 4 || size == 8 => Ok(size),
            _ => Err(format!(
                "an 'asm' operand has to have integer, floating or pointer type, not '{}'",
                self.tyname(ty)
            )),
        }
    }

    fn asm_clobber(
        &mut self,
        clobber: &ast::Spanned<String>,
        x86_64: bool,
        operands: &[ir::AsmOperand],
        rbx: Option<usize>,
    ) -> Option<Option<&'static str>> {
        let at = clobber.range;
        let name = clobber.node.trim().trim_start_matches('%');
        match name {
            "memory" | "cc" | "flags" | "dirflag" => return Some(None),
            _ => {}
        }
        if let Some(first) = rbx
            && (RBX.names.contains(&name) || RBX.high == Some(name))
        {
            self.error(
                at,
                format!(
                    "the clobber \"{name}\" is also operand {first} (\"b\") of this 'asm' \
                     statement: drop the clobber, as cinrs restores rbx after the template \
                     anyway"
                ),
            );
            return None;
        }
        if let Some(reason) = reserved_register(name) {
            self.error(
                at,
                format!("the clobber \"{name}\" is not supported: {reason}"),
            );
            return None;
        }
        let canonical = if let Some(family) = FAMILIES
            .iter()
            .find(|f| f.names.contains(&name) || f.high == Some(name))
        {
            if family.x86_64_only && !x86_64 {
                None
            } else {
                Some(if x86_64 {
                    family.names[3]
                } else {
                    family.names[2]
                })
            }
        } else {
            let limit = if x86_64 { 16 } else { 8 };
            XMM[..limit].iter().copied().find(|x| *x == name)
        };
        let Some(canonical) = canonical else {
            let hint = if name.starts_with("ymm") || name.starts_with("zmm") {
                ": cinrs maps SSE registers only; clobber the matching \"xmm\" register and \
                 note that the upper half is not declared"
            } else if name.starts_with("st") || name.starts_with("mm") {
                ": 'asm!' cannot clobber the x87 or MMX registers from here"
            } else {
                ""
            };
            self.error(
                at,
                format!("the clobber \"{name}\" is not a register cinrs can map{hint}"),
            );
            return None;
        };
        let conflicts = operands.iter().any(|operand| {
            matches!(operand.reg, AsmReg::Explicit(reg) if family_root(reg) == family_root(canonical))
        });
        if conflicts {
            self.error(
                at,
                format!("the clobber \"{name}\" is also an operand of this 'asm' statement"),
            );
            return None;
        }
        Some(Some(canonical))
    }

    /// Parses an extended template into text and operand references,
    /// reporting what cannot be mapped. `None` means an error was reported.
    fn parse_template(&mut self, template: &ast::Spanned<String>) -> Option<Vec<Piece>> {
        let at = template.range;
        let mut pieces = Vec::new();
        let mut text = String::new();
        let mut chars = template.node.chars().peekable();
        let mut ok = true;
        while let Some(c) = chars.next() {
            match c {
                '{' | '}' => {
                    self.dialect_alternatives(at);
                    return None;
                }
                '%' => {}
                _ => {
                    text.push(c);
                    continue;
                }
            }
            let Some(next) = chars.next() else {
                self.error(at, "the 'asm' template ends with a lone '%'");
                return None;
            };
            let modifier = match next {
                '%' => {
                    text.push('%');
                    continue;
                }
                '{' => {
                    text.push_str("{{");
                    continue;
                }
                '}' => {
                    text.push_str("}}");
                    continue;
                }
                '|' => {
                    text.push('|');
                    continue;
                }
                '=' => {
                    self.error(
                        at,
                        "'%=' is not supported: 'asm!' has no number unique to each instance. \
                         Use a GNU as local label ('1:' with '1b' or '1f') instead",
                    );
                    ok = false;
                    continue;
                }
                '0'..='9' | '[' => None,
                'k' | 'w' | 'b' | 'h' | 'q' => Some(next),
                'c' | 'P' | 'a' => {
                    self.error(
                        at,
                        format!(
                            "the operand modifier '%{next}' is not supported: it prints a \
                             constant or an address without its '$', which 'asm!' has no \
                             spelling for. Write the operand with \"i\" and '%0', or pass the \
                             address in a register"
                        ),
                    );
                    ok = false;
                    skip_reference(&mut chars);
                    continue;
                }
                'l' => {
                    self.error(
                        at,
                        "'%l' names an 'asm goto' label, which is not supported yet",
                    );
                    ok = false;
                    skip_reference(&mut chars);
                    continue;
                }
                other => {
                    self.error(
                        at,
                        format!("the operand modifier '%{other}' is not supported"),
                    );
                    ok = false;
                    skip_reference(&mut chars);
                    continue;
                }
            };
            let first = if modifier.is_some() {
                chars.next()
            } else {
                Some(next)
            };
            let target = match first {
                Some('[') => {
                    let mut name = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == ']' {
                            closed = true;
                            break;
                        }
                        name.push(c);
                    }
                    if !closed {
                        self.error(at, "an unterminated '%[' in the 'asm' template");
                        return None;
                    }
                    RefTarget::Name(name)
                }
                Some(d @ '0'..='9') => {
                    let mut digits = String::from(d);
                    while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(d);
                        chars.next();
                    }
                    RefTarget::Number(digits.parse().unwrap_or(usize::MAX))
                }
                _ => {
                    self.error(
                        at,
                        format!(
                            "'%{}' in the 'asm' template is not followed by an operand number",
                            modifier.unwrap_or(next)
                        ),
                    );
                    return None;
                }
            };
            if !text.is_empty() {
                pieces.push(Piece::Text(std::mem::take(&mut text)));
            }
            pieces.push(Piece::Ref { modifier, target });
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(text));
        }
        ok.then_some(pieces)
    }

    /// Writes the parsed template in `asm!`'s syntax against the operands.
    fn render_template(
        &mut self,
        pieces: &[Piece],
        template: &ast::Spanned<String>,
        names: &[Option<&str>],
        slots: &[Slot],
        operands: &mut [ir::AsmOperand],
        rbx_op: Option<usize>,
    ) -> Option<String> {
        let x86_64 = self.target.arch == Arch::X86_64;
        let at = template.range;
        // `%h` needs a register with a high byte, which is `reg_abcd`.
        for piece in pieces {
            if let Piece::Ref {
                modifier: Some('h'),
                target,
            } = piece
                && let Some(Slot::Operand(op)) = resolve(target, names, slots)
                && Some(op) != rbx_op
                && operands[op].reg == AsmReg::Class("reg")
            {
                operands[op].reg = AsmReg::Class("reg_abcd");
            }
        }
        let mut out = String::new();
        let mut ok = true;
        let mut used = vec![false; operands.len()];
        // The `"b"` operand's scratch is named by the `xchg`s around the
        // template.
        if let Some(op) = rbx_op {
            used[op] = true;
        }
        for piece in pieces {
            let (modifier, target) = match piece {
                Piece::Text(text) => {
                    out.push_str(text);
                    continue;
                }
                Piece::Ref { modifier, target } => (*modifier, target),
            };
            let slot = match resolve(target, names, slots) {
                Some(slot) => slot,
                None => {
                    let what = match target {
                        RefTarget::Number(n) => format!("'%{n}' names operand {n}"),
                        RefTarget::Name(name) => format!("'%[{name}]' names an operand"),
                    };
                    self.error(
                        at,
                        format!(
                            "{what} that this 'asm' statement does not have ({} operands)",
                            slots.len()
                        ),
                    );
                    ok = false;
                    continue;
                }
            };
            let Slot::Operand(op) = slot else {
                ok = false;
                continue;
            };
            used[op] = true;
            let operand = &operands[op];
            let size = self.size_of(operand.ty).unwrap_or(0);
            let rendered = if Some(op) == rbx_op {
                // The template runs with the value in rbx itself.
                let width = match size {
                    2 => 1,
                    4 => 2,
                    _ => 3,
                };
                render_family(&RBX, RBX.names[width], modifier, x86_64)
            } else {
                render_reference(operand, size, modifier, x86_64)
            };
            match rendered {
                Ok(text) => out.push_str(&text),
                Err(message) => {
                    self.error(at, message);
                    ok = false;
                }
            }
        }
        // GCC lets a template leave an operand out — an input only there to
        // keep a value live, an output only there to say a register changes —
        // and `asm!` calls a named operand the template never mentions an
        // error. An assembler comment mentions it without changing a byte.
        let unused: Vec<String> = operands
            .iter()
            .zip(&used)
            .filter(|(operand, used)| !**used && operand.name.is_some())
            .filter_map(|(operand, _)| {
                let size = self.size_of(operand.ty).unwrap_or(0);
                render_reference(operand, size, None, x86_64).ok()
            })
            .collect();
        if !unused.is_empty() {
            out.push_str(&format!(" /* {} */", unused.join(" ")));
        }
        ok.then_some(out)
    }
}

/// The `asm!` name of an operand the template can refer to.
fn operand_name(reg: AsmReg, index: usize) -> Option<String> {
    match reg {
        AsmReg::Class(_) => Some(format!("o{index}")),
        AsmReg::Explicit(_) => None,
    }
}

fn resolve(target: &RefTarget, names: &[Option<&str>], slots: &[Slot]) -> Option<Slot> {
    let index = match target {
        RefTarget::Number(n) => *n,
        RefTarget::Name(name) => names.iter().position(|n| *n == Some(name.as_str()))?,
    };
    slots.get(index).copied()
}

/// What one reference in the template becomes.
fn render_reference(
    operand: &ir::AsmOperand,
    size: u64,
    modifier: Option<char>,
    x86_64: bool,
) -> Result<String, String> {
    let name = operand.name.as_deref().unwrap_or_default();
    if let AsmOperandKind::Const(_) = operand.kind {
        return match modifier {
            None => Ok(format!("${{{name}}}")),
            Some(m) => Err(format!(
                "the operand modifier '%{m}' cannot apply to an immediate (\"i\") operand"
            )),
        };
    }
    match operand.reg {
        AsmReg::Explicit(reg) => {
            let family = FAMILIES
                .iter()
                .find(|f| f.names.contains(&reg))
                .expect("explicit operands are spelled from the table");
            render_family(family, reg, modifier, x86_64)
        }
        AsmReg::Class("reg_byte") => match modifier {
            None | Some('b') => Ok(format!("{{{name}}}")),
            Some(m) => Err(format!(
                "the operand modifier '%{m}' cannot apply to an 8-bit operand: 'asm!' has no \
                 wider name for a byte register. Widen the operand to 'unsigned int'"
            )),
        },
        AsmReg::Class("xmm_reg") => match modifier {
            None => Ok(format!("{{{name}}}")),
            Some(m) => Err(format!(
                "the operand modifier '%{m}' cannot apply to an SSE register (\"x\") operand"
            )),
        },
        AsmReg::Class(_) => {
            let suffix = match modifier {
                // GCC prints a register at the operand's width; `asm!` prints
                // the whole register unless a modifier says otherwise.
                None => match size {
                    2 => ":x",
                    4 => ":e",
                    _ => "",
                },
                Some('k') => ":e",
                Some('w') => ":x",
                Some('b') => ":l",
                Some('h') => ":h",
                Some('q') if x86_64 => ":r",
                Some(m) => {
                    return Err(format!(
                        "the operand modifier '%{m}' is not available on this target"
                    ));
                }
            };
            Ok(format!("{{{name}{suffix}}}"))
        }
    }
}

/// A reference to an operand that lives in one register of `family`, `reg`
/// being its name at the operand's width: the register itself, at the width
/// the modifier asks for.
fn render_family(
    family: &Family,
    reg: &str,
    modifier: Option<char>,
    x86_64: bool,
) -> Result<String, String> {
    let text = match modifier {
        None => Some(reg),
        Some('b') => Some(family.names[0]),
        Some('w') => Some(family.names[1]),
        Some('k') => Some(family.names[2]),
        Some('q') if x86_64 => Some(family.names[3]),
        Some('h') => family.high,
        Some(_) => None,
    };
    match text {
        Some(text) => Ok(format!("%{text}")),
        None => Err(format!(
            "the operand modifier '%{}' has no form for the register {reg}",
            modifier.unwrap_or(' ')
        )),
    }
}

/// The 64-bit name of the family a register belongs to, for comparing.
fn family_root(reg: &str) -> &str {
    FAMILIES
        .iter()
        .find(|f| f.names.contains(&reg) || f.high == Some(reg))
        .map_or(reg, |f| f.names[3])
}

/// Skips the operand reference after a refused modifier, so that it is not
/// reported again as text.
fn skip_reference(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if chars.peek() == Some(&'[') {
        for c in chars.by_ref() {
            if c == ']' {
                break;
            }
        }
        return;
    }
    while chars.peek().is_some_and(char::is_ascii_digit) {
        chars.next();
    }
}

fn memory_refusal(text: &str) -> String {
    format!(
        "the constraint \"{text}\" asks for a memory operand, and Rust's 'asm!' has none: pass \
         the address in a register (\"r\"(&x)) and write the memory reference in the template, \
         such as '(%0)'"
    )
}

fn letter_refusal(letter: char) -> String {
    match letter {
        'A' => "the constraint \"A\" (the edx:eax pair) is not supported: 'asm!' has no operand \
                that spans two registers. Use \"=a\" and \"=d\" with two variables and combine \
                them"
            .to_owned(),
        'f' | 't' | 'u' => format!(
            "the constraint \"{letter}\" (an x87 stack register) is not supported: 'asm!' has \
             no operand on the x87 register stack"
        ),
        'y' => {
            "the constraint \"y\" (an MMX register) is not supported: Rust has no MMX".to_owned()
        }
        'X' => "the constraint \"X\" (any operand at all) is not supported: say which register \
                class, such as \"r\""
            .to_owned(),
        'R' => "the constraint \"R\" (a legacy register) is not supported: 'asm!' has no class \
                for it. Use \"r\", or name the register"
            .to_owned(),
        'e' | 'Z' | 'I' | 'J' | 'K' | 'L' | 'M' | 'N' | 'O' | 'G' | 'C' => format!(
            "the constraint \"{letter}\" (a range-checked immediate) is not supported: write \
             \"i\", which 'asm!' takes as a 'const' operand"
        ),
        other => format!("the constraint letter '{other}' is not supported"),
    }
}
