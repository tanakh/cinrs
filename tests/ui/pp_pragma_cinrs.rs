//! `#pragma cinrs` is addressed to this crate, so an option it does not know
//! is a mistake rather than a hint meant for some other compiler. Every other
//! pragma stays silently ignored, as 6.10.6 asks.

cinrs::c99! {
    #pragma once
    #pragma omp parallel for
    #pragma cinrs unknown_option "x" //~ ERROR: unknown #pragma cinrs option 'unknown_option'
    #pragma cinrs include_path //~ ERROR: #pragma cinrs include_path needs a string literal
    #pragma cinrs link 3 //~ ERROR: #pragma cinrs link needs a string literal, found integer constant
    #pragma cinrs export "everything" //~ ERROR: unexpected string literal after #pragma cinrs export
    int x;
}

fn main() {}
