use std::{
    fmt::{self, Debug, Formatter},
};

use crate::*;
use vorbis::*;
use envelope::*;

#[derive(Clone, Copy, Default, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisInfoPsyGlobal {
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

impl Debug for VorbisInfoPsyGlobal {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisInfoPsyGlobal")
        .field("eighth_octave_lines", &self.eighth_octave_lines)
        .field("preecho_thresh", &format_args!("[{}]", format_array!(self.preecho_thresh)))
        .field("postecho_thresh", &format_args!("[{}]", format_array!(self.postecho_thresh)))
        .field("stretch_penalty", &self.stretch_penalty)
        .field("preecho_minenergy", &self.preecho_minenergy)
        .field("ampmax_att_per_sec", &self.ampmax_att_per_sec)
        .field("coupling_pkHz", &format_args!("[{}]", format_array!(self.coupling_pkHz)))
        .field("coupling_pointlimit", &format_args!("[{}, {}]",
            format_array!(self.coupling_pointlimit[0]),
            format_array!(self.coupling_pointlimit[1]),
        ))
        .field("coupling_prepointamp", &format_args!("[{}]", format_array!(self.coupling_prepointamp)))
        .field("coupling_postpointamp", &format_args!("[{}]", format_array!(self.coupling_postpointamp)))
        .field("sliding_lowpass", &format_args!("[{}, {}]",
            format_array!(self.sliding_lowpass[0]),
            format_array!(self.sliding_lowpass[1]),
        ))
        .finish()
    }
}