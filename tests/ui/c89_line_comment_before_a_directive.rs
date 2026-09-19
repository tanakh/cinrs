//! A `//` comment in a `c89!` block is diagnosed wherever the token after it
//! goes — including nowhere at all.
//!
//! C99 took the `//` comment from C++ (N644), so a strict `c89!` block has to
//! refuse one. A comment is text rather than a token, and the lexer has nowhere
//! to hang what is wrong with one but the token that follows it; when that token
//! is the `#` opening a directive it reaches no output at all, and the
//! diagnostic still has to come out.
//!
//! Two things about the blessed output are worth knowing. The caret is on the
//! *token before* the comment, because a comment is not a token and
//! `proc_macro` hands out no span for one, so the nearest anchor is what is
//! left — `tests/ui-nightly/c89_line_comment.rs` is the same case written in a
//! string literal, where a span can point at the comment itself. And each
//! annotation below *is* the comment it annotates, which is the only way to
//! write this file, since in a `c89!` block a line comment is itself a
//! diagnostic; `ui_test` strips the annotation from the line it quotes, which
//! is why the comment does not appear in the output at all.

cinrs::c89! {
    int first;      //~ ERROR: a '//' comment requires C99 or later (this block is c89!)
    #define ONE 1
    int second = ONE;

    #define TWO 2   //~ ERROR: a '//' comment requires C99 or later (this block is c89!)
    #define THREE 3
    int third = TWO + THREE;
}

fn main() {}
