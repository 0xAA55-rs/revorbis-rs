mod utils;
mod bitwise;
mod scales;
mod mdct;
mod drft;

mod headers;
mod codec;
mod blocks;
mod codebook;
mod floor;
mod mapping;
mod residue;
mod psy;
mod psy_masking;
mod bitrate;
mod envelope;
mod highlevel;

mod vorbisenc;

pub use utils::*;
pub use bitwise::*;

pub const PACKETBLOBS: usize = 15;

pub const SHOW_DEBUG: bool = false;
pub const DEBUG_ON_READ_BITS: bool = false;
pub const DEBUG_ON_WRITE_BITS: bool = false;
pub const PANIC_ON_ERROR: bool = false;

mod no_usage;

pub use headers::get_vorbis_headers_from_ogg_packet_bytes;