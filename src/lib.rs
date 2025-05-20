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
pub const PANIC_ON_ERROR: bool = true;

mod no_usage;

pub use headers::get_vorbis_headers_from_ogg_packet_bytes;

pub use codec::{VorbisInfo, VorbisDspState};

#[test]
fn test_ogg_vorbis() {
	use std::{
		fs::File,
		io::BufReader,
	};
	use ogg::OggStreamReader;
	use savagestr::prelude::*;
	let text_codecs = StringCodecMaps::new();
	let mut oggreader = OggStreamReader::new(BufReader::new(File::open("test.ogg").unwrap()));
	let (identification_header, comment_header, setup_header) = headers::read_vorbis_headers(&mut oggreader, &text_codecs).unwrap();
	dbg!(&comment_header);
	let vi = VorbisInfo::new(&identification_header, &setup_header).unwrap();
	let vd = VorbisDspState::new(vi, false).unwrap();
	dbg!(&vd);
}


