//! Evaluation bounds; the byte cap is calibrated against release checker work.

pub const MAX_INPUT_BYTES: usize = 262_144;
pub const MAX_NESTING: usize = 64;
