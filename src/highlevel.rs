#![allow(dead_code)]
#![allow(private_interfaces)]
use std::fmt::{self, Debug, Formatter};
use crate::*;
use utils::*;
use floor::VorbisFloor1;
use psy::VorbisInfoPsyGlobal;
use psy_masking::*;
use codebook::StaticCodeBook;
use residue::VorbisResidue;
use mapping::VorbisMapping;

#[derive(Default, Debug, Clone, PartialEq)]
struct StaticBookBlock {
    static_codebook: [[StaticCodeBook; 4]; 12],
}

#[derive(Default, Debug, Clone, PartialEq)]
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

#[derive(Default, Debug, Clone, PartialEq)]
struct VorbisMappingTemplate {
    map: VorbisMapping,
    res: VorbisResidueTemplate,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct VpAdjBlock {
    block: [i32; P_BANDS],
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Att3 {
    att: [i32; P_NOISECURVES],
    boost: f32,
    decay: f32,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Adj3 {
    data: [i32; P_NOISECURVES],
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
#[allow(non_snake_case)]
struct AdjStereo {
    pre: [i32; PACKETBLOBS],
    post: [i32; PACKETBLOBS],
    kHz: [f32; PACKETBLOBS],
    lowpasskHz: [f32; PACKETBLOBS],
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct NoiseGuard {
    lo: i32,
    hi: i32,
    fixed: i32,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
struct Noise3 {
    data: [[i32; 17]; P_NOISECURVES]
}

#[derive(Default, Clone, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisEncodeSetupDataTemplate {
    pub mapping: i32,
    pub rate_mapping: Vec<f64>,
    pub quality_mapping: Vec<f64>,
    pub coupling_restriction: i32,
    pub samplerate_min_restriction: i32,
    pub samplerate_max_restriction: i32,

    pub blocksize_short: Vec<i32>,
    pub blocksize_long: Vec<i32>,

    pub psy_tone_masteratt: Vec<Att3>,
    pub psy_tone_0dB: Vec<i32>,
    pub psy_tone_dBsuppress: Vec<i32>,

    pub psy_tone_adj_impulse: Vec<VpAdjBlock>,
    pub psy_tone_adj_long: Vec<VpAdjBlock>,
    pub psy_tone_adj_other: Vec<VpAdjBlock>,

    pub psy_noiseguards: Vec<NoiseGuard>,
    pub psy_noise_bias_impulse: Vec<Noise3>,
    pub psy_noise_bias_padding: Vec<Noise3>,
    pub psy_noise_bias_trans: Vec<Noise3>,
    pub psy_noise_bias_long: Vec<Noise3>,
    pub psy_noise_dBsuppress: Vec<i32>,

    pub psy_noise_compand: Vec<CommandBlock>,
    pub psy_noise_compand_short_mapping: Vec<f64>,
    pub psy_noise_compand_long_mapping: Vec<f64>,

    pub psy_noise_normal_start: [Vec<i32>; 2],
    pub psy_noise_normal_partition: [Vec<i32>; 2],
    pub psy_noise_normal_thresh: Vec<f64>,

    pub psy_ath_float: Vec<i32>,
    pub psy_ath_abs: Vec<i32>,

    pub psy_lowpass: Vec<f64>,

    pub global_params: Vec<VorbisInfoPsyGlobal>,
    pub global_mapping: Vec<f64>,
    pub stereo_modes: Vec<AdjStereo>,

    pub floor_books: Vec<Vec<Vec<StaticCodeBook>>>,
    pub floor_params: Vec<VorbisFloor1>,
    pub floor_mappings: i32,
    pub floor_mapping_list: Vec<Vec<i32>>,

    pub maps: Vec<VorbisMappingTemplate>,
}

impl Debug for VorbisEncodeSetupDataTemplate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let mut d = f.debug_struct("VorbisEncodeSetupDataTemplate");
        let d = field!(d, self, mapping);
        let d = field_array!(d, self, rate_mapping);
        let d = field_array!(d, self, quality_mapping);
        let d = field!(d, self, coupling_restriction);
        let d = field!(d, self, samplerate_min_restriction);
        let d = field!(d, self, samplerate_max_restriction);
        let d = field_array!(d, self, blocksize_short);
        let d = field_array!(d, self, blocksize_long);
        let d = field!(d, self, psy_tone_masteratt);
        let d = field_array!(d, self, psy_tone_0dB);
        let d = field_array!(d, self, psy_tone_dBsuppress);
        let d = field!(d, self, psy_tone_adj_impulse);
        let d = field!(d, self, psy_tone_adj_long);
        let d = field!(d, self, psy_tone_adj_other);
        let d = field!(d, self, psy_noiseguards);
        let d = field!(d, self, psy_noise_bias_impulse);
        let d = field!(d, self, psy_noise_bias_padding);
        let d = field!(d, self, psy_noise_bias_trans);
        let d = field!(d, self, psy_noise_bias_long);
        let d = field_array!(d, self, psy_noise_dBsuppress);
        let d = field!(d, self, psy_noise_compand);
        let d = field_array!(d, self, psy_noise_compand_short_mapping);
        let d = field_array!(d, self, psy_noise_compand_long_mapping);
        let d = d.field("psy_noise_normal_start", &NestVecFormatter::new_level1(&self.psy_noise_normal_start.to_vec()));
        let d = d.field("psy_noise_normal_partition", &NestVecFormatter::new_level1(&self.psy_noise_normal_partition.to_vec()));
        let d = field_array!(d, self, psy_noise_normal_thresh);
        let d = field_array!(d, self, psy_ath_float);
        let d = field_array!(d, self, psy_ath_abs);
        let d = field_array!(d, self, psy_lowpass);
        let d = field!(d, self, global_params);
        let d = field_array!(d, self, global_mapping);
        let d = field!(d, self, stereo_modes);
        let d = field!(d, self, floor_books);
        let d = field!(d, self, floor_params);
        let d = field!(d, self, floor_mappings);
        let d = field!(d, self, floor_mapping_list);
        let d = field!(d, self, maps);
        d.finish()
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct HighlevelByBlockType {
    pub tone_mask_setting: f64,
    pub tone_peaklimit_setting: f64,
    pub noise_bias_setting: f64,
    pub noise_compand_setting: f64,
}

#[derive(Default, Debug, Clone, PartialEq)]
#[allow(non_snake_case)]
pub struct HighlevelEncodeSetup {
    pub set_in_stone: i32,
    pub setup: VorbisEncodeSetupDataTemplate,
    pub base_setting: f64,

    pub impulse_noisetune: f64,

    /* bitrate management below all settable */
    pub req: f32,
    pub managed: i32,
    pub bitrate_min: i32,
    pub bitrate_av: i32,
    pub bitrate_av_damp: f64,
    pub bitrate_max: i32,
    pub bitrate_reservoir: i32,
    pub bitrate_reservoir_bias: f64,

    pub impulse_block_p: i32,
    pub noise_normalize_p: i32,
    pub coupling_p: i32,

    pub stereo_point_setting: f64,
    pub lowpass_kHz: f64,
    pub lowpass_altered: i32,

    pub ath_floating_dB: f64,
    pub ath_absolute_dB: f64,

    pub amplitude_track_dBpersec: f64,
    pub trigger_setting: f64,

    pub block: [HighlevelByBlockType; 4],
}
