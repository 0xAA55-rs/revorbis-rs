#![allow(dead_code)]
use crate::*;
use mdct::MdctLookup;
use codec::VorbisInfo;
use copiablebuf::CopiableBuffer;

pub const VE_PRE: usize = 16;
pub const VE_WIN: usize = 4;
pub const VE_POST: usize = 2;
pub const VE_AMP: usize = VE_PRE + VE_POST - 1;

pub const VE_BANDS: usize = 7;
pub const VE_NEARDC: usize = 15;

/// a bit less than short block
pub const VE_MINSTRETCH: usize = 2;

/// one-third full block
pub const VE_MAXSTRETCH: usize = 12;

const INIT_STORAGE: usize = 128;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
#[allow(non_snake_case)]
pub struct VorbisEnvelopeFilterState {
    pub ampbuf: [f32; 17],
    pub ampptr: usize,
    pub nearDC: [f32; 15],
    pub nearDC_acc: f32,
    pub nearDC_partialacc: f32,
    pub nearptr: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct VorbisEnvelopeBand {
    pub begin: i32,
    pub end: i32,
    pub window: CopiableBuffer<f32, 8>,
    pub total: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VorbisEnvelopeLookup {
    pub ch: i32,
    pub searchstep: i32,
    pub minenergy: f32,
    pub mdct: MdctLookup,
    pub mdct_win: Vec<f32>,
    pub band: [VorbisEnvelopeBand; VE_BANDS],
    pub filter: Vec<VorbisEnvelopeFilterState>,
    pub stretch: i32,
    pub mark: Vec<i32>,
    pub current: i32,
    pub curmark: i32,
    pub cursor: i32,
}

impl VorbisEnvelopeLookup {
    pub fn new(info: &VorbisInfo) -> Self {
        const PI: f32 = std::f32::consts::PI;
        let codec_setup = &info.codec_setup;
        let psy_g = &codec_setup.psy_g;
        let ch = info.channels;
        let mut band = [VorbisEnvelopeBand::default(); VE_BANDS];
        band[0].begin = 2;  band[0].end = 4;
        band[1].begin = 4;  band[1].end = 5;
        band[2].begin = 6;  band[2].end = 6;
        band[3].begin = 9;  band[3].end = 8;
        band[4].begin = 13; band[4].end = 8;
        band[5].begin = 17; band[5].end = 8;
        band[6].begin = 22; band[6].end = 8;
        for b in band.iter_mut() {
            let n = b.end as usize;
            b.window.resize(n, 0.0);
            for i in 0..n {
                let window = (i as f32 + 0.5) / n as f32 * PI;
                b.window[i] = window;
                b.total += window;
            }
            b.total = 1.0 / b.total;
        }
        Self {
            ch,
            searchstep: 64,
            minenergy: psy_g.preecho_minenergy,
            mdct_win: (0..INIT_STORAGE).map(|i|{let s = i as f32 / (INIT_STORAGE - 1) as f32; let s = s.sin(); s * s}).collect(),
            band,
            filter: vec![VorbisEnvelopeFilterState::default(); VE_BANDS * ch as usize],
            mark: vec![0; INIT_STORAGE],
            ..Default::default()
        }
    }
}

impl Default for VorbisEnvelopeLookup {
    fn default() -> Self {
        Self {
            ch: 0,
            searchstep: 64,
            minenergy: 0.0,
            mdct: MdctLookup::default(),
            mdct_win: Vec::default(),
            band: [VorbisEnvelopeBand::default(); VE_BANDS],
            filter: Vec::default(),
            stretch: 0,
            mark: Vec::default(),
            current: 0,
            curmark: 0,
            cursor: 0,
        }
    }
}
