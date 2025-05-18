pub mod vorbis;
pub mod bitwise;

mod utils;

mod codebook;
mod floor;
mod mapping;
mod residue;
mod psy;
mod psy_masking;
mod scales;
mod bitrate;

mod mdct;
mod drft;
mod envelope;

pub use utils::*;
pub use bitwise::*;

pub const SHOW_DEBUG: bool = false;
pub const DEBUG_ON_READ_BITS: bool = false;
pub const DEBUG_ON_WRITE_BITS: bool = false;
pub const PANIC_ON_ERROR: bool = false;

