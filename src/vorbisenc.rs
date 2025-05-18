#![allow(dead_code)]
use crate::*;
use vorbis::*;
use psych::*;
use psy_masking::*;
use floor::*;
use codebook::StaticCodeBook;
use residue::VorbisResidue;
use mapping::VorbisMapping;
use copiablebuf::CopiableBuffer;

#[derive(Default, Debug, Clone)]
struct StaticBookBlock {
	static_codebook: [[StaticCodeBook; 4]; 12],
}

#[derive(Default, Debug, Clone)]
struct VorbisResidueTemplate {
	res_type: i32,
	limit_type: i32,
	grouping: i32,
	res: VorbisResidue,
	book_aux: StaticCodeBook,
	book_aux_managed: StaticCodeBook,
	books_base: StaticBookBlock,
	books_base_managed: StaticBookBlock,
}

#[derive(Default, Debug, Clone)]
struct VorbisMappingTemplate {
	map: VorbisMapping,
	res: VorbisResidueTemplate,
}

#[derive(Default, Debug, Clone, Copy)]
struct VpAdjBlock {
	block: [i32; P_BANDS],
}

#[derive(Debug, Clone, Copy)]
struct CommandBlock {
	data: [i32; NOISE_COMPAND_LEVELS],
}

impl Default for CommandBlock {
	fn default() -> Self {
		Self {
			data: [0; NOISE_COMPAND_LEVELS],
		}
	}
}

#[derive(Default, Debug, Clone, Copy)]
struct Att3 {
	att: [i32; P_NOISECURVES],
	boost: f32,
	decay: f32,
}

#[derive(Default, Debug, Clone, Copy)]
struct Adj3 {
	data: [i32; P_NOISECURVES],
}

#[derive(Default, Debug, Clone, Copy)]
struct AdjStereo {
	pre: [i32; PACKETBLOBS],
	post: [i32; PACKETBLOBS],
	kHz: [f32; PACKETBLOBS],
	lowpasskHz: [f32; PACKETBLOBS],
}

#[derive(Default, Debug, Clone, Copy)]
struct NoiseGuard {
	lo: i32,
	hi: i32,
	fixed: i32,
}

#[derive(Default, Debug, Clone, Copy)]
struct Noise3 {
	data: [[i32; 17]; P_NOISECURVES]
}

#[derive(Default, Debug, Clone)]
#[allow(non_snake_case)]
struct VESetupDataTemplate {
	mapping: i32,
	rate_mapping: Vec<f64>,
	quality_mapping: Vec<f64>,
	coupling_restriction: i32,
	samplerate_min_restriction: i32,
	samplerate_max_restriction: i32,

	blocksize_short: Vec<i32>,
	blocksize_long: Vec<i32>,

	psy_tone_masteratt: Vec<Att3>,
	psy_tone_0dB: Vec<i32>,
	psy_tone_dBsuppress: Vec<i32>,

	psy_tone_adj_impulse: Vec<VpAdjBlock>,
	psy_tone_adj_long: Vec<VpAdjBlock>,
	psy_tone_adj_other: Vec<VpAdjBlock>,

	psy_noiseguards: Vec<NoiseGuard>,
	psy_noise_bias_impulse: Vec<Noise3>,
	psy_noise_bias_padding: Vec<Noise3>,
	psy_noise_bias_trans: Vec<Noise3>,
	psy_noise_bias_long: Vec<Noise3>,
	psy_noise_dBsuppress: Vec<i32>,

	psy_noise_compand: Vec<CommandBlock>,
	psy_noise_compand_short_mapping: Vec<f64>,
	psy_noise_compand_long_mapping: Vec<f64>,

	psy_noise_normal_start: [Vec<i32>; 2],
	psy_noise_normal_partition: [Vec<i32>; 2],
	psy_noise_normal_thresh: Vec<f64>,

	psy_ath_float: Vec<i32>,
	psy_ath_abs: Vec<i32>,

	psy_lowpass: Vec<f64>,

	global_params: Vec<VorbisInfoPsyGlobal>,
	global_mapping: Vec<f64>,
	stereo_modes: Vec<AdjStereo>,

	floor_books: Vec<Vec<Vec<StaticCodeBook>>>,
	floor_params: Vec<VorbisFloor1>,
	floor_mappings: i32,
	floor_mapping_list: Vec<Vec<i32>>,

	maps: Vec<VorbisMappingTemplate>,
}

const MODE_TEMPLATE: [VorbisMode; 2] = [
	VorbisMode {
		block_flag: false,
		window_type: 0,
		transform_type: 0,
		mapping: 0,
	},
	VorbisMode {
		block_flag: true,
		window_type: 0,
		transform_type: 0,
		mapping: 1,
	},
];

static MAP_NOMINAL: [VorbisMapping; 2] = [
	VorbisMapping {
		mapping_type: 0,
		submaps: 1,
		chmuxlist: CopiableBuffer::from_fixed_array([0, 0]),
		floorsubmap: CopiableBuffer::from_fixed_array([0]),
		residuesubmap: CopiableBuffer::from_fixed_array([0]),
		coupling_steps: 1,
		coupling_mag: CopiableBuffer::from_fixed_array([0]),
		coupling_ang: CopiableBuffer::from_fixed_array([1]),
	},
	VorbisMapping{
		mapping_type: 0,
		submaps: 1,
		chmuxlist: CopiableBuffer::from_fixed_array([0, 0]),
		floorsubmap: CopiableBuffer::from_fixed_array([1]),
		residuesubmap: CopiableBuffer::from_fixed_array([1]),
		coupling_steps: 1,
		coupling_mag: CopiableBuffer::from_fixed_array([0]),
		coupling_ang: CopiableBuffer::from_fixed_array([1]),
	},
];
