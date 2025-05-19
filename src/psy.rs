#![allow(dead_code)]
use std::{
    cmp::{min, max},
    fmt::{self, Debug, Formatter},
};

use crate::*;
use scales::*;
use psych::*;
use psy_masking::*;

#[derive(Clone, Copy, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisInfoPsy {
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

impl Debug for VorbisInfoPsy {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisInfoPsy")
        .field("block_flag", &self.block_flag)
        .field("ath_adjatt", &self.ath_adjatt)
        .field("ath_maxatt", &self.ath_maxatt)
        .field("tone_masteratt", &format_args!("[{}]", format_array!(self.tone_masteratt)))
        .field("tone_centerboost", &self.tone_centerboost)
        .field("tone_abs_limit", &self.tone_abs_limit)
        .field("toneatt", &format_args!("[{}]", format_array!(self.toneatt)))
        .field("noisemaskp", &self.noisemaskp)
        .field("noisemaxsupp", &self.noisemaxsupp)
        .field("noisewindowlo", &self.noisewindowlo)
        .field("noisewindowhi", &self.noisewindowhi)
        .field("noisewindowlomin", &self.noisewindowlomin)
        .field("noisewindowhimin", &self.noisewindowhimin)
        .field("noisewindowfixed", &self.noisewindowfixed)
        .field("noiseoff", &format_args!("[{}]", (0..P_NOISECURVES).map(|i|format!("[{}]", format_array!(self.noiseoff[i]))).collect::<Vec<_>>().join(", ")))
        .field("noisecompand", &format_args!("[{}]", format_array!(self.noisecompand)))
        .field("max_curve_dB", &self.max_curve_dB)
        .field("normal_p", &self.normal_p)
        .field("normal_start", &self.normal_start)
        .field("normal_partition", &self.normal_partition)
        .field("normal_thresh", &self.normal_thresh)
        .finish()
    }
}

impl Default for VorbisInfoPsy {
    fn default() -> Self {
        Self {
            block_flag: false,
            ath_adjatt: 0.0,
            ath_maxatt: 0.0,
            tone_masteratt: [0.0; P_NOISECURVES],
            tone_centerboost: 0.0,
            tone_decay: 0.0,
            tone_abs_limit: 0.0,
            toneatt: [0.0; P_BANDS],
            noisemaskp: 0,
            noisemaxsupp: 0.0,
            noisewindowlo: 0.0,
            noisewindowhi: 0.0,
            noisewindowlomin: 0,
            noisewindowhimin: 0,
            noisewindowfixed: 0,
            noiseoff: [[0.0; P_BANDS]; P_NOISECURVES],
            noisecompand: [0.0; NOISE_COMPAND_LEVELS],
            max_curve_dB: 0.0,
            normal_p: 0,
            normal_start: 0,
            normal_partition: 0,
            normal_thresh: 0.0,
        }
    }
}

fn min_curve(c: &mut [f32], c2: &[f32]) {
    for i in 0..EHMER_MAX {
        c[i] = c[i].min(c2[i]);
    }
}

fn max_curve(c: &mut [f32], c2: &[f32]) {
    for i in 0..EHMER_MAX {
        c[i] = c[i].max(c2[i]);
    }
}

fn attenuate_curve(c: &mut [f32], att: f32) {
    for i in 0..EHMER_MAX {
        c[i] *= att;
    }
}

#[allow(non_snake_case)]
fn setup_tone_curves(
    curveatt_dB: &[f32; P_BANDS],
    binHz: f32,
    n: usize,
    center_boost: f32,
    center_decay_rate: f32,
) -> Vec<Vec<Vec<f32>>> {
    let mut ath = [0.0; EHMER_MAX];
    let mut workc = [[[0.0; EHMER_MAX]; P_LEVELS]; P_BANDS];
    let mut athc = [[0.0; EHMER_MAX]; P_LEVELS];
    let mut ret: Vec<Vec<Vec<f32>>> = Vec::default();
    ret.resize(P_BANDS, Vec::default());

    for i in 0..P_BANDS {
        /* we add back in the ATH to avoid low level curves falling off to
           -infinity and unnecessarily cutting off high level curves in the
           curve limiting (last step). */

        /* A half-band's settings must be valid over the whole band, and
           it's better to mask too little than too much */
        let ath_offset = i * 4;
        for j in 0..EHMER_MAX {
            let mut min = 999.0_f32;
            for k in 0..4 {
                if j + k + ath_offset < MAX_ATH {
                    min = min.min(ATH[j + k + ath_offset]);
                } else {
                    min = min.min(ATH[MAX_ATH - 1]);
                }
            }
            ath[j] = min;
        }

        /* copy curves into working space, replicate the 50dB curve to 30
           and 40, replicate the 100dB curve to 110 */
        for j in 0..6 {
            workc[i][j + 2] = TONEMASKS[i][j];
        }
        workc[i][0] = TONEMASKS[i][0];
        workc[i][1] = TONEMASKS[i][0];

        /* apply centered curve boost/decay */
        for j in 0..P_LEVELS {
            for k in 0..EHMER_MAX {
                let mut adj = center_boost + (EHMER_OFFSET as f32 - k as f32).abs() * center_decay_rate;
                if adj * center_boost < 0.0 {
                    adj = 0.0;
                }
                workc[i][j][k] += adj;
            }
        }

        /* normalize curves so the driving amplitude is 0dB */
        /* make temp curves with the ATH overlayed */
        for j in 0..P_LEVELS {
            attenuate_curve(&mut workc[i][j], curveatt_dB[i] + 100.0 - max(2, j) as f32 * 10.0 - P_LEVEL_0);
            athc[j] = ath;
            attenuate_curve(&mut athc[j], 100.0 - j as f32 * 10.0 - P_LEVEL_0);
            max_curve(&mut athc[j], &workc[i][j]);
        }

        /* Now limit the louder curves.

           the idea is this: We don't know what the playback attenuation
           will be; 0dB SL moves every time the user twiddles the volume
           knob. So that means we have to use a single 'most pessimal' curve
           for all masking amplitudes, right?  Wrong.  The *loudest* sound
           can be in (we assume) a range of ...+100dB] SL.  However, sounds
           20dB down will be in a range ...+80], 40dB down is from ...+60],
           etc... */

        for j in 1..P_LEVELS {
            let &athc_j_m_1 = &athc[j - 1];
            min_curve(&mut athc[j], &athc_j_m_1);
            min_curve(&mut workc[i][j], &athc[j]);
        }
    }

    for i in 0..P_BANDS {
        let ret_i = &mut ret[i];
        ret_i.resize(P_LEVELS, Vec::default());
        /* low frequency curves are measured with greater resolution than
           the MDCT/FFT will actually give us; we want the curve applied
           to the tone data to be pessimistic and thus apply the minimum
           masking possible for a given bin.  That means that a single bin
           could span more than one octave and that the curve will be a
           composite of multiple octaves.  It also may mean that a single
           bin may span > an eighth of an octave and that the eighth
           octave values may also be composited. */

        /* which octave curves will we be compositing? */
        let bin = (fromOC!(i as f32 * 0.5) / binHz).floor();
        let lo_curve = ((toOC!(bin * binHz + 1.0) * 2.0).ceil() as usize).clamp(0, i);
        let hi_curve = min((toOC!((bin + 1.0) * binHz) * 2.0).floor() as usize, P_BANDS);

        for m in 0..P_LEVELS {
            let ret_i_m = &mut ret_i[m];
            *ret_i_m = vec![0.0; EHMER_MAX + 2];

            let mut brute_buffer = vec![999.0_f32; n];

            /* render the curve into bins, then pull values back into curve.
               The point is that any inherent subsampling aliasing results in
               a safe minimum */
            let process_curve = |k: usize, brute_buffer: &mut [f32]| {
                let mut l = 0usize;

                for j in 0..EHMER_MAX {
                    let lo_bin = ((fromOC!(j as f32 * 0.125 + k as f32 * 0.5 - 2.0625) / binHz) as usize + 0).clamp(0, n);
                    let hi_bin = ((fromOC!(j as f32 * 0.125 + k as f32 * 0.5 - 1.9375) / binHz) as usize + 1).clamp(0, n);
                    l = min(l, lo_bin);

                    while l < hi_bin && l < n {
                        brute_buffer[l] = brute_buffer[l].min(workc[k][m][j]);
                        l += 1;
                    }
                }

                while l < n {
                    brute_buffer[l] = brute_buffer[l].min(workc[k][m][EHMER_MAX - 1]);
                    l += 1;
                }
            };
            for k in lo_curve..hi_curve {
                process_curve(k, &mut brute_buffer);
            }

            /* be equally paranoid about being valid up to next half ocatve */
            if i + 1 < P_BANDS {
                let k = i + 1;
                process_curve(k, &mut brute_buffer);
            }

            for j in 0..EHMER_MAX {
                let bin = (fromOC!(j as f32 * 0.125 + i as f32 * 0.5 - 2.0) / binHz) as isize;
                ret_i_m[j + 2] = if bin < 0 {
                    -999.0
                } else if bin as usize >= n {
                    -999.0
                } else {
                    brute_buffer[bin as usize]
                };
            }

            /* add fenceposts */
            let mut j = 0;
            while j < EHMER_OFFSET {
                if ret_i_m[j + 2] > -200.0 {
                    break;
                }
                j += 1;
            }
            ret_i_m[0] = j as f32;

            j = EHMER_MAX - 1;
            while j > EHMER_OFFSET + 1 {
                if ret_i_m[j + 2] > -200.0 {
                    break;
                }
                j -= 1;
            }
            ret_i_m[1] = j as f32;
        }
    }

    ret
}

fn setup_noise_offset(rate: u32, n: usize, vorbis_info_phy: &VorbisInfoPsy) -> Vec<Vec<f32>> {
    let mut ret = vecvec![[0.0; n]; P_NOISECURVES];

    for i in 0..n {
        let halfoc = (toOC!((i as f32 + 0.5) * rate as f32 / (2.0 * n as f32)) * 2.0).clamp(0.0, (P_BANDS - 1) as f32);
        let inthalfoc = halfoc as i32;
        let del = halfoc - inthalfoc as f32;

        for j in 0..P_NOISECURVES {
            let inthalfoc = inthalfoc as usize;
            let ret_j = &mut ret[j];
            let src_j = &vorbis_info_phy.noiseoff[j];
            ret_j[i] =
                src_j[inthalfoc] * (1.0 - del) +
                src_j[inthalfoc + 1] * del;
        }
    }

    ret
}


#[derive(Clone, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisLookPsy<'a> {
    pub n: usize,
    pub vorbis_info_phy: &'a VorbisInfoPsy,

    pub tonecurves: Vec<Vec<Vec<f32>>>,
    pub noiseoffset: Vec<Vec<f32>>,

    pub ath: Vec<f32>,

    /// in n.ocshift format
    pub octave: Vec<i32>,
    pub bark: Vec<i32>,

    pub firstoc: i32,
    pub shiftoc: i32,
    pub eighth_octave_lines: i32,
    pub total_octave_lines: i32,
    pub rate: u32,

    /// Masking compensation value
    pub m_val: f32,
}

impl<'a> VorbisLookPsy<'a> {
    pub fn new(
        vorbis_info_phy: &'a VorbisInfoPsy,
        vorbis_info_psy_global: &VorbisInfoPsyGlobal,
        n: usize,
        rate: u32,
    ) -> Self {
        Self {
            
        }
    }
}

impl Debug for VorbisLookPsy<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("VorbisLookPsy")
        .field("n", &self.n)
        .field("vorbis_info_phy", &self.vorbis_info_phy)
        .field("tonecurves", &NestVecFormatter::new_level2(&self.tonecurves))
        .field("noiseoffset", &NestVecFormatter::new_level1(&self.noiseoffset))
        .field("ath", &NestVecFormatter::new(&self.ath))
        .field("octave", &NestVecFormatter::new(&self.octave))
        .field("bark", &NestVecFormatter::new(&self.bark))
        .field("firstoc", &self.firstoc)
        .field("shiftoc", &self.shiftoc)
        .field("eighth_octave_lines", &self.eighth_octave_lines)
        .field("total_octave_lines", &self.total_octave_lines)
        .field("rate", &self.rate)
        .field("m_val", &self.m_val)
        .finish()
    }
}
