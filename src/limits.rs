//! Evaluation bounds.

/// Serialized event bytes, calibrated on release builds. Of the measured shapes
/// (literal arguments, data-heavy Git pathspec substitutions and repeated
/// statements, 64 KiB through 8 MiB), substitutions were slowest: 0.318 s at
/// 256 KiB, 0.631 s at 512 KiB and 1.286 s at 1 MiB. 256 KiB was the largest
/// warm size below 0.5 s (its envelope was 262,286 bytes). These timings describe
/// those shapes on that host, not a latency guarantee.
pub const MAX_INPUT_BYTES: usize = 262_144;
pub const MAX_NESTING: usize = 64;
/// Fields in one word expansion, above the 16,000 literal operands a command
/// is checked with, so forwarding such a list through `"$@"` keeps its verdict.
pub const MAX_EXPANSION_FIELDS: usize = 16_384;
