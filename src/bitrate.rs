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
	pub managed: bool,

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

impl<W> VorbisBitrateManagerState<'_, '_, '_, W>
where
    W: Write + Debug
{
	pub fn new(vorbis_info: &VorbisInfo) -> Self {
		let codec_setup = &vorbis_info.codec_setup;
		let manager_info = &codec_setup.bitrate_manager_info;

		if manager_info.reservoir_bits > 0 {
			let ratesamples = vorbis_info.sample_rate as f32;
			let halfsamples = (vorbis_info.block_size[0] >> 1) as f32;
			let desired_fill = (manager_info.reservoir_bits as f64 * manager_info.reservoir_bias) as i32;
			Self {
				managed: true,
				short_per_long: vorbis_info.block_size[1] / vorbis_info.block_size[0],
				avg_bitsper: rint!(1.0 * manager_info.avg_rate as f32 * halfsamples / ratesamples),
				min_bitsper: rint!(1.0 * manager_info.min_rate as f32 * halfsamples / ratesamples),
				max_bitsper: rint!(1.0 * manager_info.max_rate as f32 * halfsamples / ratesamples),
				avgfloat: (PACKETBLOBS / 2) as f64,
				minmax_reservoir: desired_fill,
				avg_reservoir: desired_fill,
				vorbis_block: None,
				..Default::default()
			}
		} else {
			Self::default()
		}
	}
}

impl<W> Default for VorbisBitrateManagerState<'_, '_, '_, W>
where
    W: Write + Debug
{
	fn default() -> Self {
		use std::ptr::{write, addr_of_mut};
		let mut ret_z = mem::MaybeUninit::<Self>::zeroed();
		unsafe {
			let ptr = ret_z.as_mut_ptr();
			write(addr_of_mut!((*ptr).vorbis_block), None);
			ret_z.assume_init()
		}
	}
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct VorbisBitrateManagerInfo {
	pub avg_rate: i32,
	pub min_rate: i32,
	pub max_rate: i32,
	pub reservoir_bits: i32,
	pub reservoir_bias: f64,

	pub slew_damp: f64,
}

