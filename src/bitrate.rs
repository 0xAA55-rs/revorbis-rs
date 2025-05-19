use crate::*;
use codec::VorbisInfo;
use blocks::VorbisBlock;

#[derive(Debug, Clone)]
pub struct VorbisBitrateManagerState<'a> {
	pub managed: i32,

	pub avg_reservoir: i32,
	pub minmax_reservoir: i32,
	pub avg_bitsper: i32,
	pub min_bitsper: i32,
	pub max_bitsper: i32,

	pub short_per_long: i32,
	pub avgfloat: f64,

	pub vorbis_block: &'a VorbisBlock<'a>,
	pub choice: i32,
}

