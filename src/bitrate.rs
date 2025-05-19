#![allow(dead_code)]
use std::{
	fmt::Debug,
	mem,
	io::Write,
};

use crate::*;
use codec::VorbisInfo;
use blocks::VorbisBlock;

#[derive(Debug, Clone)]
pub struct VorbisBitrateManagerState<'a, 'b, 'c, W>
where
	W: Write + Debug
{
	pub managed: i32,

	pub avg_reservoir: i32,
	pub minmax_reservoir: i32,
	pub avg_bitsper: i32,
	pub min_bitsper: i32,
	pub max_bitsper: i32,

	pub short_per_long: i32,
	pub avgfloat: f64,

	pub vorbis_block: Option<&'a VorbisBlock<'a, 'b, 'c, W>>,
	pub choice: i32,
}

