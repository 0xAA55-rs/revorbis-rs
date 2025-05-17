pub mod vorbis;

#[macro_use]
pub mod bitwise;

pub mod mdct;
mod codebook;

pub const SHOW_DEBUG: bool = false;
pub const DEBUG_ON_READ_BITS: bool = false;
pub const DEBUG_ON_WRITE_BITS: bool = false;
pub const PANIC_ON_ERROR: bool = false;
pub mod envelope;

