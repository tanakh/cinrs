//! The caret on the comment itself, which only a span that can point inside a
//! string literal can manage: a comment is not a token, so in raw-token mode
//! the nearest anchor is the token before it — see
//! `tests/ui/c89_line_comment_before_a_directive.rs`.
//!
//! The `#define` on the next line is the point of the case. The diagnostic
//! travels on the token after the comment; that token is the `#` of a directive
//! and reaches no output at all, and this is still where it has to be reported.

//~vv ERROR: a '//' comment requires C99 or later (this block is c89!)
cinrs::c89! { r#"
    int first;      // before a directive
    #define ONE 1
    int second = ONE;
"# }

fn main() {}
