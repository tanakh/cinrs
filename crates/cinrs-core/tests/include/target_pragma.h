/* A `#pragma cinrs target` where it cannot work: the scan that applies one
 * reads the unit's own text, before preprocessing, so a header's is seen only
 * by the preprocessor — too late for the predefined macros it would have
 * changed. `tests/targets.rs` checks that this is reported rather than
 * silently ignored. */
#pragma cinrs target "i686-unknown-linux-gnu"
