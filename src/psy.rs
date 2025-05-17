#![allow(dead_code)]
use std::{
    fmt::{self, Debug, Formatter},
};

const P_BANDS: usize = 17;
const P_LEVELS: usize = 8;
const P_LEVEL_0: f32 = 30.0;
const P_NOISECURVES: usize = 3;

const NOISE_COMPAND_LEVELS: usize = 40;

use crate::*;
use vorbis::*;
use envelope::*;

#[derive(Clone, Copy, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisPsy {
	pub block_flag: bool,

	pub ath_adjatt: f32,
	pub ath_maxatt: f32,

	pub tone_masteratt: [f32; P_NOISECURVES],
	pub tone_centerboost: f32,
	pub tone_decay: f32,
	pub tone_abs_limit: f32,
	pub toneatt: [f32; P_BANDS],

	pub noisemaskp: i32,
	pub noisemaxsupp: f32,
	pub noisewindowlo: f32,
	pub noisewindowhi: f32,
	pub noisewindowlomin: i32,
	pub noisewindowhimin: i32,
	pub noisewindowfixed: i32,
	pub noiseoff: [[f32; P_BANDS]; P_NOISECURVES],
	pub noisecompand: [f32; NOISE_COMPAND_LEVELS],

	pub max_curve_dB: f32,

	pub normal_p: i32,
	pub normal_start: i32,
	pub normal_partition: i32,
	pub normal_thresh: f64,
}

impl Debug for VorbisPsy {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		f.debug_struct("VorbisPsy")
		.field("block_flag", &self.block_flag)
		.field("ath_adjatt", &self.ath_adjatt)
		.field("ath_maxatt", &self.ath_maxatt)
		.field("tone_masteratt", &format_args!("[{}]", format_array!(self.tone_masteratt, ", ", "{}")))
		.field("tone_centerboost", &self.tone_centerboost)
		.field("tone_abs_limit", &self.tone_abs_limit)
		.field("toneatt", &format_args!("[{}]", format_array!(self.toneatt, ", ", "{}")))
		.field("noisemaskp", &self.noisemaskp)
		.field("noisemaxsupp", &self.noisemaxsupp)
		.field("noisewindowlo", &self.noisewindowlo)
		.field("noisewindowhi", &self.noisewindowhi)
		.field("noisewindowlomin", &self.noisewindowlomin)
		.field("noisewindowhimin", &self.noisewindowhimin)
		.field("noisewindowfixed", &self.noisewindowfixed)
		.field("noiseoff", &format_args!("[{}]", (0..P_NOISECURVES).map(|i|format!("[{}]", format_array!(self.noiseoff[i], ", ", "{}"))).collect::<Vec<_>>().join(", ")))
		.field("noisecompand", &format_args!("[{}]", format_array!(self.noisecompand, ", ", "{}")))
		.field("max_curve_dB", &self.max_curve_dB)
		.field("normal_p", &self.normal_p)
		.field("normal_start", &self.normal_start)
		.field("normal_partition", &self.normal_partition)
		.field("normal_thresh", &self.normal_thresh)
		.finish()
	}
}

impl Default for VorbisPsy {
	fn default() -> Self {
		Self {
			block_flag: false,
			ath_adjatt: 0.0,
			ath_maxatt: 0.0,
			tone_masteratt: [0f32; P_NOISECURVES],
			tone_centerboost: 0.0,
			tone_decay: 0.0,
			tone_abs_limit: 0.0,
			toneatt: [0f32; P_BANDS],
			noisemaskp: 0,
			noisemaxsupp: 0.0,
			noisewindowlo: 0.0,
			noisewindowhi: 0.0,
			noisewindowlomin: 0,
			noisewindowhimin: 0,
			noisewindowfixed: 0,
			noiseoff: [[0f32; P_BANDS]; P_NOISECURVES],
			noisecompand: [0f32; NOISE_COMPAND_LEVELS],
			max_curve_dB: 0.0,
			normal_p: 0,
			normal_start: 0,
			normal_partition: 0,
			normal_thresh: 0.0,
		}
	}
}

#[derive(Clone, Copy, Default, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisPsyGlobal {
	pub eighth_octave_lines: i32,

	/* for block long/short tuning; encode only */
	pub preecho_thresh: [f32; VE_BANDS],
	pub postecho_thresh: [f32; VE_BANDS],
	pub stretch_penalty: f32,
	pub preecho_minenergy: f32,

	pub ampmax_att_per_sec: f32,

	/* channel coupling config */
	pub coupling_pkHz: [i32; PACKETBLOBS],
	pub coupling_pointlimit: [[i32; PACKETBLOBS]; 2],
	pub coupling_prepointamp: [i32; PACKETBLOBS],
	pub coupling_postpointamp: [i32; PACKETBLOBS],
	pub sliding_lowpass: [[i32; PACKETBLOBS]; 2],
}

impl Debug for VorbisPsyGlobal {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		f.debug_struct("VorbisPsyGlobal")
		.field("eighth_octave_lines", &self.eighth_octave_lines)
		.field("preecho_thresh", &format_args!("[{}]", format_array!(self.preecho_thresh, ", ", "{}")))
		.field("postecho_thresh", &format_args!("[{}]", format_array!(self.postecho_thresh, ", ", "{}")))
		.field("stretch_penalty", &self.stretch_penalty)
		.field("preecho_minenergy", &self.preecho_minenergy)
		.field("ampmax_att_per_sec", &self.ampmax_att_per_sec)
		.field("coupling_pkHz", &format_args!("[{}]", format_array!(self.coupling_pkHz, ", ", "{}")))
		.field("coupling_pointlimit", &format_args!("[{}, {}]",
			format_array!(self.coupling_pointlimit[0], ", ", "{}"),
			format_array!(self.coupling_pointlimit[1], ", ", "{}"),
		))
		.field("coupling_prepointamp", &format_args!("[{}]", format_array!(self.coupling_prepointamp, ", ", "{}")))
		.field("coupling_postpointamp", &format_args!("[{}]", format_array!(self.coupling_postpointamp, ", ", "{}")))
		.field("sliding_lowpass", &format_args!("[{}, {}]",
			format_array!(self.sliding_lowpass[0], ", ", "{}"),
			format_array!(self.sliding_lowpass[1], ", ", "{}"),
		))
		.finish()
	}
}
