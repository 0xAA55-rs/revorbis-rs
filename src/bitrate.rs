use crate::*;
use vorbis::*;

#[derive(Debug, Clone)]
pub struct VorbisBitrateManagerState<'a> {
	managed: i32,

	avg_reservoir: i32,
	minmax_reservoir: i32,
	avg_bitsper: i32,
	min_bitsper: i32,
	max_bitsper: i32,

	short_per_long: i32,
	avgfloat: f64,

	vorbis_block: &'a VorbisBlock<'a>,
	choice: i32,
}

