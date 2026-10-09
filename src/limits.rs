//! Evaluation bounds; the byte cap is calibrated against release checker work.

pub const MAX_INPUT_BYTES: usize = 262_144;
pub const MAX_NESTING: usize = 64;
/// Fields in one word expansion, above the 16,000 literal operands a command
/// is checked with, so forwarding such a list through `"$@"` keeps its verdict.
pub const MAX_EXPANSION_FIELDS: usize = 16_384;
